use super::*;
use crate::file_interface::{
    FileExport, FileExportKind, InterfaceBodyChannel, InterfaceCallable, InterfaceCanonicalBody,
    InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceTypeSnapshot,
};
use crate::mir::{MirBlock, MirExternalFunctionIdentity, MirStatement, MirTerminator};

#[test]
fn substitute_function_rewrites_generic_call_type_args() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    let mut function = MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: DefId(1), type_args: vec![generic] },
                    args: Vec::new(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut state = PackageInstantiate {
        types: &mut types,
        interner: &interner,
        struct_metadata: None,
        templates: FxHashMap::default(),
        generic_params: FxHashMap::default(),
        tensor_limes_templates: FxHashMap::default(),
        tensor_edge_policy_providers: Vec::new(),
        function_ids_by_source: FxHashMap::default(),
        require_tensor_edge_resolution: true,
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id: MirFunctionId(1),
        errors: Vec::new(),
        created: Vec::new(),
        imported_devices: ImportedDeviceIndex::default(),
        device_registrations: Vec::new(),
        registered_device_keys: FxHashSet::default(),
        synthetic_templates: FxHashMap::default(),
    };
    let mut bindings = MirGenericBindings::default();
    bindings.insert(symbol, concrete);

    assert!(state.substitute_function(&mut function, &bindings, Span::default()));
    let MirStatementKind::Call { callee, .. } = &function.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    let MirCallee::Definition { type_args, .. } = callee else {
        panic!("expected definition callee");
    };
    assert_eq!(type_args, &[concrete]);
}

// DFV2-3 imported role + body channel: fixtures for the imported-instance
// linkage. One generic template owned by an imported module (source DefId 7)
// called with a concrete type argument from the consumer's program.

const IMPORTED_SOURCE: DefId = DefId(7);

fn imported_device_route(body: InterfaceBodyChannel) -> ImportedDeviceRoute {
    ImportedDeviceRoute {
        identity: MirExternalFunctionIdentity::new("probe", vec!["src".to_owned()], "scala"),
        contract: InterfaceDeviceContract {
            role: InterfaceDeviceRole::Kernel,
            visibility: InterfaceDeviceVisibility::Public,
            body,
        },
    }
}

/// Shared fixture tuple: types, interner, consumer program, templates, generic params.
type DeviceFixture = (
    TypeTable,
    Interner,
    MirProgram,
    Vec<MirFunction>,
    Vec<(DefId, Vec<MirGenericParam>)>,
);

fn imported_device_fixture() -> DeviceFixture {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(IMPORTED_SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: generic,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: generic,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let caller = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: IMPORTED_SOURCE, type_args: vec![concrete] },
                    args: Vec::new(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let program = MirProgram { functions: vec![caller] };
    let generic_params = vec![(
        IMPORTED_SOURCE,
        vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
    )];
    (types, interner, program, vec![template], generic_params)
}

fn imported_device_index(route: ImportedDeviceRoute) -> ImportedDeviceIndex {
    ImportedDeviceIndex::from_routes([(IMPORTED_SOURCE, route)])
}

/// DFV2-3 (b): a concrete instantiation of an imported generic device
/// function registers the imported device function for the consumer.
#[test]
fn imported_device_instantiation_registers_the_device_function() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Source));

    let (monomorphizations, registrations) = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect("imported device instantiation");

    assert_eq!(monomorphizations.len(), 1, "one concrete instance is created");
    let registration = registrations
        .first()
        .expect("the imported device instance is registered for the consumer");
    assert_eq!(registration.key, monomorphizations[0].key);
    assert_eq!(registration.function, monomorphizations[0].function);
    assert_eq!(registration.route.identity.name, "scala");
    assert_eq!(registration.route.contract.role, InterfaceDeviceRole::Kernel);
    assert_eq!(registration.route.contract.visibility, InterfaceDeviceVisibility::Public);
    assert_eq!(registration.route.contract.body, InterfaceBodyChannel::Source);

    // The consumer's call site now targets the concrete instance.
    let caller = &program.functions[0];
    let MirStatementKind::Call { callee, .. } = &caller.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    assert_eq!(*callee, MirCallee::Function(registration.function));
}

/// DFV2-3 (e, f): a signature-only imported device function fails closed with
/// a named reason before composition — the role bit alone never validates the
/// call.
#[test]
fn signature_only_imported_device_fails_closed_with_named_reason() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Missing));

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect_err("signature-only device import must fail closed");

    // Issue-code identity only: MirError carries the route as rendered prose,
    // and non-renderer tests must not assert message text (hygiene:
    // `non_renderer_tests_do_not_assert_diagnostic_text`).
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_body_missing"),
        "expected the named imported_device_body_missing reason, got {errors:?}"
    );
    // The unresolved call was not rewritten to a valid target.
    let caller = &program.functions[0];
    let MirStatementKind::Call { callee, .. } = &caller.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    assert!(
        matches!(callee, MirCallee::Definition { source, .. } if *source == IMPORTED_SOURCE),
        "a role bit alone must not make the unresolved call valid"
    );
}

/// SGR-U0: an imported device instantiation whose size bindings cannot be
/// resolved from the use site fails closed with the stable classified issue
/// `imported_device_bindings_unresolved` — never a silent provider fallback.
#[test]
fn unresolved_imported_device_bindings_fail_closed_with_named_reason() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let size_symbol = interner.intern("M");
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    // Template admits no bindable witness for its declared shape param, so
    // the call site resolves no binding for `M`.
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(IMPORTED_SOURCE),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let caller = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: IMPORTED_SOURCE, type_args: Vec::new() },
                    args: Vec::new(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut program = MirProgram { functions: vec![caller] };
    let generic_params = vec![(
        IMPORTED_SOURCE,
        vec![MirGenericParam { name: size_symbol, is_shape: true, runtime_witness: None }],
    )];
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Source));

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &[template],
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect_err("unresolved imported device bindings must fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_bindings_unresolved"),
        "expected the named imported_device_bindings_unresolved reason, got {errors:?}"
    );
    // No registration and no valid target: the call site keeps its
    // unresolved Definition callee.
    let caller = &program.functions[0];
    let MirStatementKind::Call { callee, .. } = &caller.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    assert!(
        matches!(callee, MirCallee::Definition { source, .. } if *source == IMPORTED_SOURCE),
        "an unresolved binding must not make the device call valid"
    );
}

/// DFV2-3 (d): a serialized canonical-body fixture reaches the consumer's
/// composition input unchanged — same template bytes, same body hash.
#[test]
fn canonical_body_fixture_reaches_composition_input_unchanged() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let template_bytes = b"canonical-body-template-probe::scala".to_vec();
    let canonical = InterfaceCanonicalBody::new(template_bytes.clone());
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Canonical(
        InterfaceCanonicalBody::new(template_bytes.clone()),
    )));

    let (_monomorphizations, registrations) = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect("imported device instantiation with a canonical body");

    let registration = registrations.first().expect("registered instance");
    let InterfaceBodyChannel::Canonical(received) = &registration.route.contract.body else {
        panic!(
            "the canonical body channel must reach the composition input, got {:?}",
            registration.route.contract.body
        );
    };
    assert_eq!(received.template, canonical.template, "template bytes unchanged");
    assert_eq!(received.body_hash, canonical.body_hash, "body hash unchanged");
}

/// DFV2-3: the transport is additive — the legacy entry point keeps its
/// behavior for callers that supply no imported-device index.
#[test]
fn legacy_instantiation_without_device_index_is_unchanged() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();

    let monomorphizations =
        instantiate_merged_generic_calls(&mut program, &templates, &generic_params, &mut types, &interner, None)
            .expect("legacy instantiation");

    assert_eq!(monomorphizations.len(), 1);
    let caller = &program.functions[0];
    let MirStatementKind::Call { callee, .. } = &caller.blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    assert_eq!(*callee, MirCallee::Function(monomorphizations[0].function));
}

/// DFV2-3 (e), artifact-restored shape: a signature-only device export riding
/// an interface with no parseable source body — the state package artifacts
/// and legacy restored interfaces produce — routes into the consumer and
/// fails closed with the named reason at instantiation.
#[test]
fn artifact_restored_signature_only_device_route_fails_closed() {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "scala".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: Vec::new(),
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: Some(InterfaceDeviceContract {
                role: InterfaceDeviceRole::Kernel,
                visibility: InterfaceDeviceVisibility::Public,
                body: InterfaceBodyChannel::Missing,
            }),
        }),
    });

    let routes = ImportedDeviceRoute::from_interface("probe", &["src".to_owned()], &interface);
    assert_eq!(routes.len(), 1, "a Missing-channel device export stays a route");
    let route = routes.into_iter().next().expect("route");

    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let index = ImportedDeviceIndex::from_routes([(IMPORTED_SOURCE, route)]);

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect_err("a signature-only device route must fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_body_missing"),
        "expected the named imported_device_body_missing reason, got {errors:?}"
    );
}

// DFV2-4 merged-pipeline composition: an imported device instance whose body
// calls another imported device function composes here, between shape
// finalization and the caller's ValidatedMir rebuild.

const IMPORTED_INNER_SOURCE: DefId = DefId(8);

fn imported_device_pair_fixture() -> DeviceFixture {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let probe = interner.intern("probe");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    let imported_inner = MirFunction {
        id: MirFunctionId(0),
        source: Some(IMPORTED_INNER_SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: generic,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0))))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: generic,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let imported_outer = MirFunction {
        id: MirFunctionId(0),
        source: Some(IMPORTED_SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: generic,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: vec![crate::mir::MirLocal {
            id: crate::mir::MirLocalId(1),
            name: None,
            ty: generic,
            mutable: false,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::local(crate::mir::MirLocalId(1))),
                    callee: MirCallee::Definition { source: IMPORTED_INNER_SOURCE, type_args: vec![generic] },
                    args: vec![MirOperand::Constant(crate::mir::MirConstant::String(probe))],
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(1))))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: generic,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let caller = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: IMPORTED_SOURCE, type_args: vec![concrete] },
                    args: Vec::new(),
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let program = MirProgram { functions: vec![caller] };
    let generic_params = vec![
        (
            IMPORTED_SOURCE,
            vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
        ),
        (
            IMPORTED_INNER_SOURCE,
            vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
        ),
    ];
    (types, interner, program, vec![imported_outer, imported_inner], generic_params)
}

fn imported_device_pair_index() -> ImportedDeviceIndex {
    ImportedDeviceIndex::from_routes([
        (
            IMPORTED_SOURCE,
            ImportedDeviceRoute {
                identity: MirExternalFunctionIdentity::new("probe", vec!["src".to_owned()], "scala"),
                contract: InterfaceDeviceContract {
                    role: InterfaceDeviceRole::Kernel,
                    visibility: InterfaceDeviceVisibility::Public,
                    body: InterfaceBodyChannel::Source,
                },
            },
        ),
        (
            IMPORTED_INNER_SOURCE,
            ImportedDeviceRoute {
                identity: MirExternalFunctionIdentity::new("probe", vec!["src".to_owned()], "inner"),
                contract: InterfaceDeviceContract {
                    role: InterfaceDeviceRole::Kernel,
                    visibility: InterfaceDeviceVisibility::Private,
                    body: InterfaceBodyChannel::Source,
                },
            },
        ),
    ])
}

/// DFV2-4 (a)/(d)/(h): the imported outer instance's device call to the
/// imported inner device function composes at the merged seam; the composed
/// private helper leaves no standalone symbol and no surviving device call.
#[test]
fn imported_device_call_composes_at_merged_seam() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_pair_fixture();
    let index = imported_device_pair_index();

    let (monomorphizations, registrations) = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect("imported device pair instantiation + composition");

    assert_eq!(registrations.len(), 2, "both imported instances register for the consumer");
    let device_roles = index
        .iter()
        .map(|(source, _route)| (*source, crate::mir::MirDeviceRole::Kernel))
        .collect::<FxHashMap<_, _>>();
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls_with_metadata(
            &program.functions,
            &monomorphizations,
            &device_roles,
            &crate::mir::MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "no device call may survive merged composition: {:#?}",
        program.functions
    );
    assert!(
        program
            .functions
            .iter()
            .all(|function| function.source != Some(IMPORTED_INNER_SOURCE)),
        "the composed private imported helper must be pruned: {:#?}",
        program.functions
    );
    assert!(
        program
            .functions
            .iter()
            .any(|function| function.source == Some(IMPORTED_SOURCE)),
        "the public imported entry must stay a program symbol"
    );
}

/// DFV2-4 (e): a concrete cycle across imported device templates reports the
/// full chain at the merged seam.
#[test]
fn imported_device_cycle_is_rejected_with_full_chain() {
    let (mut types, mut interner, mut program, mut templates, generic_params) = imported_device_pair_fixture();
    // Rewrite the inner template's body to call back into the outer source.
    if let Some(inner) = templates
        .iter_mut()
        .find(|template| template.source == Some(IMPORTED_INNER_SOURCE))
    {
        inner.locals.push(crate::mir::MirLocal {
            id: crate::mir::MirLocalId(1),
            name: None,
            ty: inner.return_ty,
            mutable: false,
            span: Span::default(),
        });
        inner.blocks[0].statements.push(MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(MirPlace::local(crate::mir::MirLocalId(1))),
                callee: MirCallee::Definition { source: IMPORTED_SOURCE, type_args: Vec::new() },
                args: vec![MirOperand::Constant(crate::mir::MirConstant::String(
                    interner.intern("back"),
                ))],
            },
            span: Span::default(),
        });
        inner.blocks[0].terminator = MirTerminator {
            kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(1))))),
            span: Span::default(),
        };
    }
    let index = imported_device_pair_index();

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &std::collections::HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect_err("an imported device cycle must fail closed");

    // Issue-code identity only: the structured chain/cycle args are asserted
    // in fragment_composition_test.rs, where FragmentCompositionError.args
    // are visible; the MirError seam does not carry them (hygiene:
    // `non_renderer_tests_do_not_assert_diagnostic_text`).
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "nucleum_fragment_cycle"),
        "expected the named cycle issue at the merged seam, got {errors:?}"
    );
}

// CDC-1: the consumer units' own device roles and launch entries join the
// package-seam composition as the leaf-tail union.

const LOCAL_CALLER_SOURCE: DefId = DefId(11);
const LOCAL_TEMPLATE_SOURCE: DefId = DefId(12);

/// A consumer-local public Kernel caller calling a local generic Kernel
/// helper template — the consumer-local-only shape (no imported routes).
fn consumer_local_fixture() -> DeviceFixture {
    let (types, interner, program, templates, mut generic_params) = imported_device_fixture();
    // Re-key the fixture onto consumer-local sources and give the caller the
    // public-Kernel declaration identity composition keys on.
    let mut templates = templates;
    templates[0].source = Some(LOCAL_TEMPLATE_SOURCE);
    let mut program = program;
    let MirStatementKind::Call { callee, .. } = &mut program.functions[0].blocks[0].statements[0].kind else {
        panic!("expected call statement");
    };
    let MirCallee::Definition { source, .. } = callee else {
        panic!("expected definition callee");
    };
    *source = LOCAL_TEMPLATE_SOURCE;
    program.functions[0].source = Some(LOCAL_CALLER_SOURCE);
    let generic_params = vec![(LOCAL_TEMPLATE_SOURCE, generic_params.remove(0).1)];
    (types, interner, program, templates, generic_params)
}

fn consumer_local_roles() -> FxHashMap<DefId, crate::mir::MirDeviceRole> {
    let mut roles = FxHashMap::default();
    roles.insert(LOCAL_CALLER_SOURCE, crate::mir::MirDeviceRole::Kernel);
    roles.insert(LOCAL_TEMPLATE_SOURCE, crate::mir::MirDeviceRole::Kernel);
    roles
}

/// CDC-1 (g) + design-ruling point 4: consumer-local-only inputs reach the
/// one existing composition pass (the union keys the gate), a consumer-local
/// caller's local call composes, and the composed private helper is pruned.
#[test]
fn consumer_local_only_package_composes_at_the_union_gate() {
    let (mut types, interner, mut program, templates, generic_params) = consumer_local_fixture();

    let (monomorphizations, registrations) = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &ImportedDeviceIndex::from_routes([]),
        &{
            let mut roles = std::collections::HashMap::new();
            for (def, role) in consumer_local_roles() {
                roles.insert(def, role);
            }
            roles
        },
        &std::iter::once(LOCAL_CALLER_SOURCE).collect(),
    )
    .expect("consumer-local-only composition");

    assert!(registrations.is_empty(), "no imported routes means no registrations");
    assert!(
        crate::mir::fragment_composition::surviving_fragment_calls_with_metadata(
            &program.functions,
            &monomorphizations,
            &consumer_local_roles(),
            &crate::mir::MirDeviceFunctionMetadataMap::default(),
        )
        .is_empty(),
        "the consumer-local caller's device call must compose: {:#?}",
        program.functions
    );
    assert!(
        program
            .functions
            .iter()
            .all(|function| function.source != Some(LOCAL_TEMPLATE_SOURCE)),
        "the composed private local helper must be pruned: {:#?}",
        program.functions
    );
    assert!(
        program
            .functions
            .iter()
            .any(|function| function.source == Some(LOCAL_CALLER_SOURCE)),
        "the consumer-local launch entry must stay a program symbol"
    );
}

/// CDC-1 (d): a callee that is also a consumer-local launch entry survives
/// composition — the launch-entry union prevents pruning a public Kernel
/// instance its caller composed into.
#[test]
fn consumer_local_launch_entry_union_prevents_pruning() {
    let (mut types, interner, mut program, templates, generic_params) = consumer_local_fixture();

    let (_monomorphizations, _registrations) = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &ImportedDeviceIndex::from_routes([]),
        &{
            let mut roles = std::collections::HashMap::new();
            for (def, role) in consumer_local_roles() {
                roles.insert(def, role);
            }
            roles
        },
        &[LOCAL_CALLER_SOURCE, LOCAL_TEMPLATE_SOURCE]
            .into_iter()
            .collect(),
    )
    .expect("consumer-local composition with a launchable callee");

    assert!(
        program
            .functions
            .iter()
            .any(|function| function.source == Some(LOCAL_TEMPLATE_SOURCE)),
        "the public Kernel callee stays a program symbol after its caller composed: {:#?}",
        program.functions
    );
}

/// CDC-1 fail-closed union: a consumer-local role contradicting an imported
/// route contract on one source never silently overrides — the named
/// identity fires before composition.
#[test]
fn consumer_local_role_conflict_fails_closed() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Source));

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &[(IMPORTED_SOURCE, crate::mir::MirDeviceRole::Fragment)]
            .into_iter()
            .collect(),
        &std::collections::HashSet::new(),
    )
    .expect_err("a contradictory role union must fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "consumer_device_role_conflict"),
        "expected the named consumer_device_role_conflict reason, got {errors:?}"
    );
}

/// CDC-1 (e) budget clause: a consumer-local caller expanding a device chain
/// deeper than the documented depth budget fails closed with the same named
/// identity imported-instance callers hit — `nucleum_fragment_expansion`.
/// (Source-level imported chains are blocked by the pre-existing template
/// merge collapse; the observed/limit args are pinned in
/// `fragment_composition_test.rs`, which sees `FragmentCompositionError`.)
#[test]
fn consumer_local_caller_hits_expansion_budget_identity() {
    // A chain of generic device templates t0..t17 (depth > the documented
    // budget of 16): t_i calls t_{i-1}; the consumer-local Kernel caller
    // calls t17. Same shapes as the proven imported-pair fixture, chained.
    const CHAIN_DEPTH: usize = 18;
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let probe = interner.intern("probe");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let concrete = MirType::semantic(types.primitive(Primitive::Textus));
    let chain_sources: Vec<DefId> = (0..CHAIN_DEPTH)
        .map(|index| DefId(100 + index as u32))
        .collect();
    let caller_source = DefId(99);
    let mut templates = Vec::new();
    let mut generic_params = Vec::new();
    for (index, source) in chain_sources.iter().enumerate() {
        let mut template = MirFunction {
            id: MirFunctionId(index as u32),
            source: Some(*source),
            name: None,
            params: vec![crate::mir::MirParam {
                local: crate::mir::MirLocalId(0),
                name: None,
                ty: generic,
                mode: crate::mir::MirParamMode::Owned,
                span: Span::default(),
            }],
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: Vec::new(),
            return_ty: generic,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        };
        if index == 0 {
            template.blocks = vec![MirBlock {
                id: crate::mir::MirBlockId(0),
                statements: Vec::new(),
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(
                        0,
                    ))))),
                    span: Span::default(),
                },
                span: Span::default(),
            }];
        } else {
            template.locals.push(crate::mir::MirLocal {
                id: crate::mir::MirLocalId(1),
                name: None,
                ty: generic,
                mutable: false,
                span: Span::default(),
            });
            template.blocks = vec![MirBlock {
                id: crate::mir::MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: Some(MirPlace::local(crate::mir::MirLocalId(1))),
                        callee: MirCallee::Definition { source: chain_sources[index - 1], type_args: vec![generic] },
                        args: vec![MirOperand::Constant(crate::mir::MirConstant::String(probe))],
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(
                        1,
                    ))))),
                    span: Span::default(),
                },
                span: Span::default(),
            }];
        }
        templates.push(template);
        generic_params.push((
            *source,
            vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
        ));
    }
    let caller = MirFunction {
        id: MirFunctionId(90),
        source: Some(caller_source),
        name: None,
        params: Vec::new(),
        locals: vec![crate::mir::MirLocal {
            id: crate::mir::MirLocalId(1),
            name: None,
            ty: concrete,
            mutable: false,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::local(crate::mir::MirLocalId(1))),
                    callee: MirCallee::Definition { source: chain_sources[CHAIN_DEPTH - 1], type_args: vec![concrete] },
                    args: vec![MirOperand::Constant(crate::mir::MirConstant::String(probe))],
                },
                span: Span::default(),
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(1))))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: concrete,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut program = MirProgram { functions: vec![caller] };

    let consumer_roles = std::iter::once((caller_source, crate::mir::MirDeviceRole::Kernel))
        .chain(
            chain_sources
                .iter()
                .map(|source| (*source, crate::mir::MirDeviceRole::Kernel)),
        )
        .collect::<std::collections::HashMap<_, _>>();

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &ImportedDeviceIndex::from_routes([]),
        &consumer_roles,
        &std::iter::once(caller_source).collect(),
    )
    .expect_err("a consumer-local depth overflow must fail closed at the seam");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "nucleum_fragment_expansion"),
        "expected the named nucleum_fragment_expansion identity, got {errors:?}"
    );
}

/// CDC-1 (e): with consumer-local roles present at the seam, a signature-only
/// imported route still fails closed with the same named identity an
/// imported-instance caller hits — the union never widens admission.
#[test]
fn consumer_local_roles_do_not_widen_signature_only_admission() {
    let (mut types, interner, mut program, templates, generic_params) = imported_device_fixture();
    let index = imported_device_index(imported_device_route(InterfaceBodyChannel::Missing));

    let errors = instantiate_merged_generic_calls_with_devices(
        &mut program,
        &templates,
        &generic_params,
        &mut types,
        &interner,
        None,
        &index,
        &[(LOCAL_CALLER_SOURCE, crate::mir::MirDeviceRole::Kernel)]
            .into_iter()
            .collect(),
        &std::iter::once(LOCAL_CALLER_SOURCE).collect(),
    )
    .expect_err("a signature-only device route must still fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_body_missing"),
        "expected the named imported_device_body_missing reason, got {errors:?}"
    );
}

/// MMG-F1 risk proof: two concrete shapes using one generic template must
/// produce two instances, while structurally duplicate index nodes for one
/// shape must reuse the canonical key and instance. Bindings are insert-only;
/// a later call must never overwrite the first shape or silently merge [8, 8]
/// into [2, 2].
#[test]
fn two_shape_instances_keep_canonical_keys_and_do_not_merge() {
    const SOURCE: DefId = DefId(77);
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let element = types.primitive(Primitive::Fractus);
    let shape_param_a = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(501)));
    let shape_param_b = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(502)));
    let template_shape = types.intern_index(IndexExpr::Tuple(vec![shape_param_a, shape_param_b]));
    let stale_param_a = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(601)));
    let stale_param_b = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(602)));
    let stale_shape = types.intern_index(IndexExpr::Tuple(vec![stale_param_a, stale_param_b]));
    let template_tensor = MirType::semantic(types.tensor_with_shape(element, template_shape));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: template_tensor,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: vec![crate::mir::MirLocal {
            id: crate::mir::MirLocalId(1),
            name: None,
            ty: MirType::semantic(element),
            mutable: false,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![
                MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: None,
                        call: crate::mir::MirRuntimeCall {
                            intrinsic: crate::mir::MirIntrinsic::Collection(crate::mir::MirCollectionOp::TensorMean),
                            args: vec![MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0)))],
                            return_ty: MirType::semantic(element),
                        },
                    },
                    span: Span::default(),
                },
                MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(crate::mir::MirLocalId(1)),
                        value: crate::mir::MirValue {
                            id: crate::mir::MirValueId(0),
                            kind: crate::mir::MirValueKind::Operand(MirOperand::Constant(
                                crate::mir::MirConstant::ShapeSize { shape: stale_shape, axis: None },
                            )),
                            ty: MirType::semantic(element),
                            span: Span::default(),
                        },
                    },
                    span: Span::default(),
                },
            ],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: template_tensor,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };

    let make_shape = |row: u64, column: u64, types: &mut TypeTable| {
        let row = types.intern_index(IndexExpr::Literal(row));
        let column = types.intern_index(IndexExpr::Literal(column));
        types.intern_index(IndexExpr::Tuple(vec![row, column]))
    };
    let shape_2x2_a = make_shape(2, 2, &mut types);
    let shape_2x2_b = make_shape(2, 2, &mut types);
    assert_ne!(shape_2x2_a, shape_2x2_b, "fixture must use duplicate raw index identities");
    let shape_8x8 = make_shape(8, 8, &mut types);
    let tensor_2x2_a = MirType::semantic(types.tensor_with_shape(element, shape_2x2_a));
    let tensor_2x2_b = MirType::semantic(types.tensor_with_shape(element, shape_2x2_b));
    let tensor_8x8 = MirType::semantic(types.tensor_with_shape(element, shape_8x8));

    let make_caller = |id: u32, tensor: MirType| MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![crate::mir::MirLocal {
            id: crate::mir::MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Call {
                    destination: None,
                    callee: MirCallee::Definition { source: SOURCE, type_args: Vec::new() },
                    args: vec![MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0)))],
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut program = MirProgram {
        functions: vec![
            make_caller(1, tensor_2x2_a),
            make_caller(2, tensor_2x2_b),
            make_caller(3, tensor_8x8),
        ],
    };
    let generic_params = vec![(
        SOURCE,
        vec![
            MirGenericParam { name: crate::lexer::Symbol(501), is_shape: true, runtime_witness: None },
            MirGenericParam { name: crate::lexer::Symbol(502), is_shape: true, runtime_witness: None },
        ],
    )];

    let monomorphizations =
        instantiate_merged_generic_calls(&mut program, &[template], &generic_params, &mut types, &interner, None)
            .expect("distinct concrete shape instances must link");

    assert_eq!(monomorphizations.len(), 2, "duplicate [2, 2] key must reuse one instance");
    assert_eq!(
        monomorphizations
            .iter()
            .filter(|entry| entry.key.shape_args == monomorphizations[0].key.shape_args)
            .count(),
        1,
        "the instance index must contain one canonical key per concrete shape"
    );
    assert_ne!(
        monomorphizations[0].key.shape_args, monomorphizations[1].key.shape_args,
        "same-template [2, 2] and [8, 8] keys must stay distinct"
    );
    let target = |function: &MirFunction| {
        let MirStatementKind::Call { callee, .. } = &function.blocks[0].statements[0].kind else {
            panic!("expected generic call statement");
        };
        let MirCallee::Function(target) = callee else {
            panic!("generic call must be rewritten to an instance");
        };
        *target
    };
    assert_eq!(target(&program.functions[0]), target(&program.functions[1]));
    assert_ne!(target(&program.functions[0]), target(&program.functions[2]));
    for monomorphization in &monomorphizations {
        let instance = program
            .functions
            .iter()
            .find(|function| function.id == monomorphization.function)
            .expect("each canonical shape key has an instance");
        assert!(
            !function_contains_shape_size(&mut instance.clone()),
            "package finalization must fold every generated-companion ShapeSize"
        );
    }
}

/// sgr C2b: minting an instance from a generic template substitutes a
/// body-visible `magnitudo` operand — the symbolic `ShapeSize { Param(d) }`
/// C2a lowers for `rope_norm<d>(0)` — to the concrete integer bound by the
/// call site's size witness. The shared template keeps its symbolic operand
/// (copy-not-move posture); only the minted instance concretizes.
#[test]
fn minted_instance_substitutes_shape_size_param_to_concrete_integer() {
    const SOURCE: DefId = DefId(78);
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let element = types.primitive(Primitive::Fractus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let param_row = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(710)));
    let param_dim = types.intern_index(IndexExpr::Param(crate::lexer::Symbol(711)));
    let template_shape = types.intern_index(IndexExpr::Tuple(vec![param_row, param_dim]));
    let template_tensor = MirType::semantic(types.tensor_with_shape(element, template_shape));
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: template_tensor,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: crate::mir::MirRuntimeCall {
                        intrinsic: crate::mir::MirIntrinsic::Collection(crate::mir::MirCollectionOp::TensorRope),
                        args: vec![
                            MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0))),
                            MirOperand::Constant(crate::mir::MirConstant::Int(0)),
                            MirOperand::Constant(crate::mir::MirConstant::ShapeSize { shape: param_dim, axis: None }),
                        ],
                        return_ty: MirType::semantic(vacuum),
                    },
                },
                span: Span::default(),
            }],
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };

    let row = types.intern_index(IndexExpr::Literal(8));
    let dim = types.intern_index(IndexExpr::Literal(64));
    let concrete_shape = types.intern_index(IndexExpr::Tuple(vec![row, dim]));
    let concrete_tensor = MirType::semantic(types.tensor_with_shape(element, concrete_shape));
    let mut program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(1),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![crate::mir::MirLocal {
                id: crate::mir::MirLocalId(0),
                name: None,
                ty: concrete_tensor,
                mutable: false,
                span: Span::default(),
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: crate::mir::MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Definition { source: SOURCE, type_args: Vec::new() },
                        args: vec![MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0)))],
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let generic_params = vec![(
        SOURCE,
        vec![
            MirGenericParam { name: crate::lexer::Symbol(710), is_shape: true, runtime_witness: None },
            MirGenericParam { name: crate::lexer::Symbol(711), is_shape: true, runtime_witness: None },
        ],
    )];

    let (monomorphizations, _) = instantiate_merged_generic_calls_with_devices_and_metadata(
        &mut program,
        std::slice::from_ref(&template),
        &generic_params,
        &mut types,
        &interner,
        None,
        &mut MirDeviceFunctionMetadataMap::default(),
        &ImportedDeviceIndex::from_routes([]),
        &HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .expect("the rope template must instantiate with the concrete size witness");

    assert_eq!(monomorphizations.len(), 1, "one call site mints one instance");
    let instance = program
        .functions
        .iter()
        .find(|function| function.id == monomorphizations[0].function)
        .expect("the minted instance joins the program");
    let rope_calls = instance
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => Some(call),
            _ => None,
        })
        .filter(|call| {
            matches!(
                call.intrinsic,
                crate::mir::MirIntrinsic::Collection(crate::mir::MirCollectionOp::TensorRope)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(rope_calls.len(), 1, "one tensor-rope call in the instance");
    assert!(
        matches!(
            rope_calls[0].args.get(1),
            Some(MirOperand::Constant(crate::mir::MirConstant::Int(0)))
        ),
        "rope position stays the literal 0: {:?}",
        rope_calls[0].args
    );
    assert!(
        matches!(rope_calls[0].args.get(2), Some(MirOperand::Constant(crate::mir::MirConstant::Int(dim))) if *dim == 64),
        "instantiated rope dim must be the concrete integer from the size binding: {:?}",
        rope_calls[0].args
    );
    assert!(
        !function_contains_shape_size(&mut instance.clone()),
        "no symbolic shape-size operand survives instantiation"
    );
    let template_rope_dim = template
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => call.args.get(2).cloned(),
            _ => None,
        })
        .expect("the template keeps its rope statement");
    assert!(
        matches!(template_rope_dim, MirOperand::Constant(crate::mir::MirConstant::ShapeSize { shape, axis: None }) if shape == param_dim),
        "the shared template keeps its symbolic operand (copy-not-move): {template_rope_dim:?}"
    );
}

#[test]
fn runtime_shape_key_distinguishes_caller_formal_axes_from_local_bindings() {
    // A nested size-generic call arrives at the key derivation with its size
    // argument already resolved to the calling instance's formal tensor axis;
    // a top-level call with a runtime-shaped argument arrives with a local
    // binding extent guarded by a call-site TensorShapeCheck. The first keys
    // per caller (the instance signature then matches the caller's argument
    // type); the second collapses to the callee's formal carrier witness.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let height = interner.intern("H");
    let caller_axis = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter: 1,
        axis: 0,
    }));
    let local_binding = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::TensorBindingAxis {
        binding: DefId(3),
        axis: 0,
    }));
    let formal_witness = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter: 0,
        axis: 0,
    }));
    let mut state = PackageInstantiate {
        types: &mut types,
        interner: &interner,
        struct_metadata: None,
        templates: FxHashMap::default(),
        generic_params: FxHashMap::default(),
        tensor_limes_templates: FxHashMap::default(),
        tensor_edge_policy_providers: Vec::new(),
        function_ids_by_source: FxHashMap::default(),
        require_tensor_edge_resolution: true,
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id: MirFunctionId(1),
        errors: Vec::new(),
        created: Vec::new(),
        imported_devices: ImportedDeviceIndex::default(),
        device_registrations: Vec::new(),
        registered_device_keys: FxHashSet::default(),
        synthetic_templates: FxHashMap::default(),
    };
    let params = vec![MirGenericParam { name: height, is_shape: true, runtime_witness: Some(formal_witness) }];

    let mut nested_bindings = MirShapeBindings::default();
    nested_bindings.insert(height, caller_axis);
    let nested_key = state
        .key_from_bindings(
            DefId(7),
            &params,
            &MirGenericBindings::default(),
            &nested_bindings,
            &MirShapeBindings::default(),
            Span::default(),
        )
        .expect("nested caller-axis binding keys without error");
    assert_eq!(
        nested_key.shape_args,
        vec![caller_axis],
        "a caller formal-axis binding keys the instance by that axis"
    );

    let mut top_level_bindings = MirShapeBindings::default();
    top_level_bindings.insert(height, local_binding);
    let top_level_key = state
        .key_from_bindings(
            DefId(7),
            &params,
            &MirGenericBindings::default(),
            &top_level_bindings,
            &MirShapeBindings::default(),
            Span::default(),
        )
        .expect("local runtime binding keys without error");
    assert_eq!(
        top_level_key.shape_args,
        vec![formal_witness],
        "a local runtime binding collapses to the callee's formal witness"
    );
}

// ── A-139: a generic callee's alternate-exit type binds from the catch place ──

/// `mayfail<E>() → numerus ⇥ E` called through a `TryCall` whose error place is
/// a concrete `textus` local, with no explicit type arguments and no other
/// witness for `E`. The error place is the only fact that fixes `E`.
#[test]
fn try_call_error_place_binds_the_generic_error_type() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("E");
    let error_param = MirType::semantic(types.intern(Type::Param(symbol)));
    let textus = MirType::semantic(types.primitive(Primitive::Textus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let callee_source = DefId(9);
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(callee_source),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
            span: Span::default(),
        }],
        return_ty: vacuum,
        error_ty: Some(error_param),
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let block = |id: u32, kind: MirTerminatorKind| MirBlock {
        id: crate::mir::MirBlockId(id),
        statements: Vec::new(),
        terminator: MirTerminator { kind, span: Span::default() },
        span: Span::default(),
    };
    let caller = MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![crate::mir::MirLocal {
            id: crate::mir::MirLocalId(0),
            name: None,
            ty: textus,
            mutable: true,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: vec![
            block(
                0,
                MirTerminatorKind::TryCall {
                    destination: None,
                    callee: MirCallee::Definition { source: callee_source, type_args: Vec::new() },
                    args: Vec::new(),
                    ok_block: crate::mir::MirBlockId(1),
                    error_place: MirPlace::local(crate::mir::MirLocalId(0)),
                    error_block: crate::mir::MirBlockId(2),
                },
            ),
            block(1, MirTerminatorKind::Return(None)),
            block(2, MirTerminatorKind::Return(None)),
        ],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut program = MirProgram { functions: vec![caller] };
    let generic_params = vec![(
        callee_source,
        vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
    )];

    let monomorphizations =
        instantiate_merged_generic_calls(&mut program, &[template], &generic_params, &mut types, &interner, None)
            .unwrap_or_else(|errors| panic!("the catch place must fix E; link errors: {errors:?}"));

    assert_eq!(monomorphizations.len(), 1, "one concrete instance is created");
    assert_eq!(
        monomorphizations[0].key.type_args,
        vec![textus],
        "E binds to the catch place type"
    );
    let MirTerminatorKind::TryCall { callee, .. } = &program.functions[0].blocks[0].terminator.kind else {
        panic!("expected the try call terminator");
    };
    assert_eq!(*callee, MirCallee::Function(monomorphizations[0].function));
}

/// A-137: an unsigned MIR constant argument binds a generic witness at the
/// sized `u64`, the same type the single-unit monomorphizer and the lowerer
/// give `MirConstant::UInt`; plain `numerus` is the signed `i64` default.
#[test]
fn uint_constant_argument_binds_the_sized_u64_witness() {
    const SOURCE: DefId = DefId(79);
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("T");
    let generic = MirType::semantic(types.intern(Type::Param(symbol)));
    let u64_ty = types.sized_numeric(Primitive::Numerus, crate::semantic::NumericWidth::U64);
    let plain = types.primitive(Primitive::Numerus);
    assert_ne!(u64_ty, plain, "the fixture needs a sized u64 distinct from plain numerus");
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(SOURCE),
        name: None,
        params: vec![crate::mir::MirParam {
            local: crate::mir::MirLocalId(0),
            name: None,
            ty: generic,
            mode: crate::mir::MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: crate::mir::MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(crate::mir::MirLocalId(0))))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: generic,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(1),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: crate::mir::MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Definition { source: SOURCE, type_args: Vec::new() },
                        args: vec![MirOperand::Constant(crate::mir::MirConstant::UInt(7))],
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let generic_params = vec![(
        SOURCE,
        vec![MirGenericParam { name: symbol, is_shape: false, runtime_witness: None }],
    )];

    let monomorphizations = instantiate_merged_generic_calls(
        &mut program,
        std::slice::from_ref(&template),
        &generic_params,
        &mut types,
        &interner,
        None,
    )
    .expect("a uint constant argument must bind the generic witness");

    assert_eq!(monomorphizations.len(), 1, "one call site mints one instance");
    let witness = monomorphizations[0].key.type_args[0].semantic_id();
    assert_eq!(
        witness, u64_ty,
        "a uint constant argument must bind the sized u64 witness, not plain numerus"
    );
}

/// A-133: one failed size-only explicit type argument reports exactly one
/// diagnostic. The explicit NATURAL arg `9` never interns an
/// `IndexExpr::Literal(9)` in this fixture, so `explicit_shape_witness` fails
/// closed; the witness helper and its caller used to each push a `MirError`
/// for the same failure (the helper's specific "unresolved type parameter"
/// plus the caller's own "does not match parameter kind").
#[test]
fn failed_size_only_explicit_type_arg_reports_once() {
    const SOURCE: DefId = DefId(88);
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let nine = interner.intern("9");
    let explicit_nine = MirType::semantic(types.intern(Type::Param(nine)));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let template = MirFunction {
        id: MirFunctionId(0),
        source: Some(SOURCE),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    };
    let mut program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(1),
            source: None,
            name: None,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Definition { source: SOURCE, type_args: vec![explicit_nine] },
                        args: Vec::new(),
                    },
                    span: Span::default(),
                }],
                terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    };
    let generic_params = vec![(
        SOURCE,
        vec![MirGenericParam { name: Symbol(701), is_shape: true, runtime_witness: None }],
    )];

    let errors =
        instantiate_merged_generic_calls(&mut program, &[template], &generic_params, &mut types, &interner, None)
            .expect_err("a NATURAL size argument with no interned extent must fail closed");

    assert_eq!(
        errors.len(),
        1,
        "one failed explicit size argument reports exactly once: {errors:?}"
    );
    assert_eq!(
        errors[0].issue, "invalid_mir_generic_call_signature_has_unresolved_type_parameter",
        "the more specific witness diagnostic wins: {errors:?}"
    );
}
