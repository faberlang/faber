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
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id: MirFunctionId(1),
        errors: Vec::new(),
        created: Vec::new(),
        imported_devices: ImportedDeviceIndex::default(),
        device_registrations: Vec::new(),
        registered_device_keys: FxHashSet::default(),
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

fn imported_device_fixture() -> (
    TypeTable,
    Interner,
    MirProgram,
    Vec<MirFunction>,
    Vec<(DefId, Vec<MirGenericParam>)>,
) {
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
    let program = MirProgram { functions: vec![caller], ..MirProgram::default() };
    let generic_params = vec![(IMPORTED_SOURCE, vec![MirGenericParam { name: symbol, is_shape: false }])];
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
    )
    .expect_err("signature-only device import must fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_body_missing"),
        "expected the named imported_device_body_missing reason, got {errors:?}"
    );
    let named = errors
        .iter()
        .find(|error| error.issue == "imported_device_body_missing")
        .expect("named reason");
    assert!(
        named.message.contains("probe::src::scala"),
        "the named reason must name the provider-route identity: {}",
        named.message
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
            type_param_constraints: Vec::new(),
            params: Vec::new(),
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
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
    )
    .expect_err("a signature-only device route must fail closed");

    assert!(
        errors
            .iter()
            .any(|error| error.issue == "imported_device_body_missing" && error.message.contains("probe::src::scala")),
        "expected the named imported_device_body_missing reason naming the route, got {errors:?}"
    );
}
