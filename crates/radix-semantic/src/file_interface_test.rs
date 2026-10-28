//! S0-R1: interface snapshots serialize kinded applied size arguments.

use crate::file_interface::{
    FileInterfaceError, InterfaceAppliedArg, InterfaceCallable, InterfaceCallableParam,
    InterfaceIndexSnapshot, InterfaceNominalKind, InterfaceNominalType,
    InterfaceTypeParamConstraint, InterfaceTypeSnapshot, import_interface_callable_with_resolver,
    import_interface_type, snapshot_interface_callable_with_resolver, snapshot_interface_type,
};
use crate::{
    FuncSig, GenericParamDescriptor, GenericParamSort, IndexExpr, InferVar, NumericWidth,
    ParamType, Primitive, Resolver, Type, TypeTable,
};
use radix_lexer::Interner;
use radix_syntax::GenericParamKind;
use radix_types::AppliedArg;

/// One unconstrained, default-free `magnitudo` descriptor for call fixtures.
fn magnitudo_param(symbol: radix_lexer::Symbol) -> GenericParamDescriptor {
    GenericParamDescriptor {
        name: symbol,
        sort: GenericParamSort::Magnitudo,
        constraint: crate::TypeParamConstraint::Any,
        default: None,
    }
}

#[test]
fn file_interface_preserves_generic_size_literal_round_trip() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(3));
    let base = types.vector_with_width(elem, width);
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(width)]));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot applied size");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Literal(
            3
        ))]
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import applied size");

    let Type::Applied(_, args) = target_types.get(imported) else {
        panic!("expected imported applied type");
    };
    assert_eq!(args.len(), 1);
    match &args[0] {
        AppliedArg::Size(index) => {
            assert!(matches!(
                target_types.get_index(*index),
                IndexExpr::Literal(3)
            ));
        }
        other @ AppliedArg::Type(_) => panic!("expected size argument, got {other:?}"),
    }
}

#[test]
fn file_interface_preserves_generic_size_param_round_trip() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let m = interner.intern("M");
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Param(m));
    let base = types.vector_with_width(elem, width);
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(width)]));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot applied size param");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Param(
            "M".to_owned()
        ))]
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import applied size param");

    let Type::Applied(_, args) = target_types.get(imported) else {
        panic!("expected imported applied type");
    };
    match &args[0] {
        AppliedArg::Size(index) => {
            assert!(
                matches!(target_types.get_index(*index), IndexExpr::Param(sym) if target_interner.resolve(*sym) == "M")
            );
        }
        other @ AppliedArg::Type(_) => panic!("expected size argument, got {other:?}"),
    }
}

#[test]
fn file_interface_preserves_generic_kinded_args_in_order() {
    // A mixed application `[Type(textus), Size(4)]` keeps argument kind and
    // order across the snapshot boundary; sizes never become fake types.
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let textus = types.primitive(Primitive::Textus);
    let width = types.intern_index(IndexExpr::Literal(4));
    let base = types.array(textus);
    let applied = types.intern(Type::Applied(
        base,
        vec![AppliedArg::Type(textus), AppliedArg::Size(width)],
    ));

    let snapshot =
        snapshot_interface_type(applied, &types, &interner).expect("snapshot mixed args");

    let InterfaceTypeSnapshot::Applied(_, args) = &snapshot else {
        panic!("expected applied snapshot, got {snapshot:?}");
    };
    assert_eq!(
        args,
        &vec![
            InterfaceAppliedArg::Type(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
            InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Literal(4)),
        ]
    );
}

// ── PKG-REP: typed kernel-twin signature representability ──

/// Build the typed-twin `FuncSig` shape used by the gradus kernel twins
/// (`fn linear<size M, size K, size N>(tensor<f32, [M, K]> x, …)`): ordered
/// size parameters with symbolic tensor-shape witnesses.
fn twin_sig(types: &mut TypeTable, interner: &mut Interner) -> FuncSig {
    let m = interner.intern("M");
    let k = interner.intern("K");
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_m = types.intern_index(IndexExpr::Param(m));
    let dim_k = types.intern_index(IndexExpr::Param(k));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim_m, dim_k]));
    let tensor = types.tensor_with_shape(elem, shape);
    FuncSig {
        type_params: vec![magnitudo_param(m), magnitudo_param(k)],
        params: vec![ParamType {
            ty: tensor,
            mode: crate::SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: tensor,
        err: None,
        is_async: false,
        is_generator: false,
    }
}

/// The twin signature snapshots as an `InterfaceCallable` whose ordered size
/// parameters and `InterfaceIndexSnapshot::Param` shape witnesses survive
/// verbatim, with the device contract left unpopulated (KPC-WIRE owns that
/// parity; PKG-REP's exclusion clause).
#[test]
fn twin_callable_snapshot_preserves_size_params_and_shape_witnesses() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let sig = twin_sig(&mut types, &mut interner);
    let resolver = Resolver::new();

    let callable = snapshot_interface_callable_with_resolver(&sig, &types, &interner, &resolver)
        .expect("twin signature must be representable");

    let names: Vec<&str> = callable
        .type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(names, vec!["M", "K"]);
    assert!(
        callable
            .type_params
            .iter()
            .all(|param| param.kind == GenericParamKind::Magnitudo)
    );
    assert_eq!(
        callable.params[0].ty,
        InterfaceTypeSnapshot::Tensor(
            Box::new(InterfaceTypeSnapshot::SizedNumeric(
                Primitive::Fractus,
                NumericWidth::F32
            )),
            InterfaceIndexSnapshot::Tuple(vec![
                InterfaceIndexSnapshot::Param("M".to_owned()),
                InterfaceIndexSnapshot::Param("K".to_owned()),
            ])
        )
    );
    assert_eq!(*callable.ret, callable.params[0].ty);
    assert!(callable.device.is_none());
}

/// The snapshotted twin round trips back into a consumer `FuncSig`: the size
/// parameters re-intern by name and the shape witnesses reconstruct as
/// `IndexExpr::Param` terms, so producer identity is preserved through the
/// portable interface rather than guessed.
#[test]
fn twin_callable_round_trips_param_witnesses_to_consumer_tables() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let sig = twin_sig(&mut types, &mut interner);
    let resolver = Resolver::new();
    let callable = snapshot_interface_callable_with_resolver(&sig, &types, &interner, &resolver)
        .expect("twin signature must be representable");

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_callable_with_resolver(
        &callable,
        &mut target_types,
        &mut target_interner,
        &resolver,
    )
    .expect("twin callable must import");

    let imported_params: Vec<String> = imported
        .type_params
        .iter()
        .map(|param| target_interner.resolve(param.name).to_owned())
        .collect();
    assert_eq!(imported_params, vec!["M".to_owned(), "K".to_owned()]);
    let Type::Tensor(elem, shape) = target_types.get(imported.ret) else {
        panic!(
            "expected imported tensor return, got {:?}",
            target_types.get(imported.ret)
        );
    };
    assert_eq!(
        target_types.get(*elem),
        &Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
    );
    match target_types.get_index(*shape) {
        IndexExpr::Tuple(dims) => {
            assert_eq!(dims.len(), 2);
            for (dim, name) in dims.iter().zip(["M", "K"]) {
                match target_types.get_index(*dim) {
                    IndexExpr::Param(symbol) => {
                        assert_eq!(target_interner.resolve(*symbol), name);
                    }
                    other => panic!("expected Param witness for {name}, got {other:?}"),
                }
            }
        }
        other => panic!("expected tuple shape, got {other:?}"),
    }
}

/// A portable callable stores the raw declared return; importing re-applies
/// the wrapper the async/generator flags declare, so the consumer's `FuncSig`
/// matches the exporter's.
#[test]
fn async_callable_import_wraps_return_in_promissum_failable() {
    let callable = InterfaceCallable {
        type_params: Vec::new(),
        tensor_edge_policy_requirements: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
        err: Some(Box::new(InterfaceTypeSnapshot::Primitive(
            Primitive::Textus,
        ))),
        is_async: true,
        is_generator: false,
        device: None,
    };
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(
        &InterfaceTypeSnapshot::Func(callable),
        &mut types,
        &mut interner,
    )
    .expect("import async failable callable");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected imported callable");
    };
    let Type::PromissumFailable(success, alternate) = types.get(sig.ret) else {
        panic!(
            "expected promissum failable return, got {:?}",
            types.get(sig.ret)
        );
    };
    assert_eq!(*success, types.primitive(Primitive::Numerus));
    assert_eq!(*alternate, types.primitive(Primitive::Textus));
    assert_eq!(sig.err, None);
}

#[test]
fn async_generator_callable_import_wraps_return_in_async_cursor() {
    let callable = InterfaceCallable {
        type_params: Vec::new(),
        tensor_edge_policy_requirements: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
        err: Some(Box::new(InterfaceTypeSnapshot::Primitive(
            Primitive::Textus,
        ))),
        is_async: true,
        is_generator: true,
        device: None,
    };
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(
        &InterfaceTypeSnapshot::Func(callable),
        &mut types,
        &mut interner,
    )
    .expect("import async generator callable");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected imported callable");
    };
    let Type::AsyncCursor(item, alternate) = types.get(sig.ret) else {
        panic!("expected async cursor return, got {:?}", types.get(sig.ret));
    };
    assert_eq!(*item, types.primitive(Primitive::Numerus));
    assert_eq!(*alternate, types.primitive(Primitive::Textus));
    assert_eq!(sig.err, None);
    assert!(sig.is_async);
    assert!(sig.is_generator);
}

#[test]
fn generator_callable_import_wraps_return_in_array() {
    let callable = InterfaceCallable {
        type_params: Vec::new(),
        tensor_edge_policy_requirements: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
        err: None,
        is_async: false,
        is_generator: true,
        device: None,
    };
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(
        &InterfaceTypeSnapshot::Func(callable),
        &mut types,
        &mut interner,
    )
    .expect("import generator callable");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected imported callable");
    };
    let Type::Array(item) = types.get(sig.ret) else {
        panic!("expected array return, got {:?}", types.get(sig.ret));
    };
    assert_eq!(*item, types.primitive(Primitive::Numerus));
}

/// Fail-closed pins (PKG-REP `done_when` (d)): inference holes, error types,
/// and still-inferring tensor indexes stay unrepresentable at the seam even
/// though symbolic `Param` witnesses are accepted.
#[test]
fn unrepresentable_type_and_index_shapes_fail_closed() {
    let mut types = TypeTable::new();
    let interner = Interner::new();

    let infer = types.intern(Type::Infer(InferVar(0)));
    assert_eq!(
        snapshot_interface_type(infer, &types, &interner),
        Err(FileInterfaceError::InferenceTypeUnsupported)
    );

    let error = types.intern(Type::Error);
    assert_eq!(
        snapshot_interface_type(error, &types, &interner),
        Err(FileInterfaceError::ErrorTypeUnsupported)
    );

    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let hole = types.intern_index(IndexExpr::Infer(crate::IndexVar(1)));
    let tensor = types.tensor_with_shape(elem, hole);
    assert_eq!(
        snapshot_interface_type(tensor, &types, &interner),
        Err(FileInterfaceError::IndexInferenceUnsupported)
    );

    let local_runtime = types.intern_index(IndexExpr::Runtime(
        radix_types::RuntimeExtentId::ScalarBinding(radix_hir::DefId(77)),
    ));
    let local_tensor = types.tensor_with_shape(elem, local_runtime);
    assert_eq!(
        snapshot_interface_type(local_tensor, &types, &interner),
        Err(FileInterfaceError::IndexInferenceUnsupported)
    );

    // A nominal without a resolver stays unsupported rather than guessed.
    let nominal = types.intern(Type::Struct(radix_hir::DefId(1)));
    assert_eq!(
        snapshot_interface_type(nominal, &types, &interner),
        Err(FileInterfaceError::NominalTypeUnsupported {
            kind: crate::file_interface::InterfaceNominalKind::Struct
        })
    );
}

#[test]
fn file_interface_round_trips_portable_runtime_and_affine_indices() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let runtime = types.intern_index(IndexExpr::Runtime(
        radix_types::RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 1,
        },
    ));
    let minus_one = radix_types::add_index_constant(&mut types, runtime, -1)
        .expect("symbolic offset remains a term");
    let divided = radix_types::divide_index_exact(&mut types, minus_one, 2)
        .expect("symbolic exactness remains an obligation");
    let tensor = types.tensor_with_shape(elem, divided);

    let snapshot = snapshot_interface_type(tensor, &types, &interner)
        .expect("portable runtime index snapshot");
    let InterfaceTypeSnapshot::Tensor(_, shape) = &snapshot else {
        panic!("expected tensor snapshot, got {snapshot:?}");
    };
    assert_eq!(
        shape,
        &InterfaceIndexSnapshot::Divide {
            numerator: Box::new(InterfaceIndexSnapshot::Affine {
                base: Box::new(InterfaceIndexSnapshot::RuntimeTensorParameterAxis {
                    parameter: 0,
                    axis: 1,
                }),
                offset: -1,
            }),
            divisor: 2,
        }
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import runtime index");
    let Type::Tensor(_, imported_shape) = target_types.get(imported) else {
        panic!("expected imported tensor");
    };
    assert!(matches!(
        target_types.get_index(*imported_shape),
        IndexExpr::Divide { divisor: 2, .. }
    ));
}

#[test]
fn file_interface_rejects_known_non_integral_index_arithmetic() {
    let interner = Interner::new();
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let five = types.intern_index(IndexExpr::Literal(5));
    let invalid = types.intern_index(IndexExpr::Divide {
        numerator: five,
        divisor: 2,
    });
    let tensor = types.tensor_with_shape(elem, invalid);

    assert_eq!(
        snapshot_interface_type(tensor, &types, &interner),
        Err(FileInterfaceError::InvalidIndexArithmetic(
            radix_types::IndexArithmeticError::NonIntegral
        ))
    );
}

/// N7E-1: the policy-float cell snapshots and imports back to the same
/// structural type, for both policies and a float width of each family.
#[test]
fn policy_float_file_interface_round_trip() {
    use radix_types::FloatPolicy;

    let interner = Interner::new();
    let mut types = TypeTable::new();
    let cases = [
        (FloatPolicy::Trapping, NumericWidth::F32),
        (FloatPolicy::Trapping, NumericWidth::F64),
        (FloatPolicy::Saturating, NumericWidth::F16),
        (FloatPolicy::Saturating, NumericWidth::BF16),
    ];
    for (policy, width) in cases {
        let cell = types.intern(Type::PolicyFloat(policy, width));
        let snapshot = snapshot_interface_type(cell, &types, &interner).expect("snapshot cell");
        assert_eq!(snapshot, InterfaceTypeSnapshot::PolicyFloat(policy, width));

        let mut target_types = TypeTable::new();
        let mut target_interner = Interner::new();
        let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
            .expect("import cell");
        assert_eq!(
            target_types.get(imported),
            &Type::PolicyFloat(policy, width)
        );
    }
}

#[test]
fn file_interface_default_remaps_embedded_targets_and_text_slots() {
    use crate::file_interface::InterfaceFieldDefault;
    use radix_hir::{DefId, HirArrayElement, HirExpression, HirExpressionKind, HirId, HirLiteral};
    use radix_lexer::Span;
    let mut source_types = TypeTable::new();
    let mut source_interner = Interner::new();
    let text = source_interner.intern("producer text");
    let string = source_types.primitive(Primitive::Textus);
    let array = source_types.array(string);
    let literal = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::String(text)),
        ty: Some(string),
        span: Span::default(),
    };
    let expression = HirExpression {
        id: HirId(1),
        ty: Some(array),
        span: Span::default(),
        kind: HirExpressionKind::Verte {
            source: Box::new(HirExpression {
                id: HirId(2),
                ty: Some(array),
                span: Span::default(),
                kind: HirExpressionKind::Array(vec![HirArrayElement::Expr(literal)]),
            }),
            target: array,
            entries: None,
        },
    };
    let snapshot = InterfaceFieldDefault::snapshot(
        &expression,
        &source_types,
        &source_interner,
        &Resolver::new(),
        None,
    )
    .expect("snapshot default");
    let mut target_types = TypeTable::new();
    let collision = target_types.intern(Type::Enum(DefId(20)));
    assert_eq!(
        array, collision,
        "the producer array slot is a consumer enum slot"
    );
    let mut target_interner = Interner::new();
    target_interner.intern("unrelated consumer text");
    let imported = snapshot
        .import(&mut target_types, &mut target_interner, &Resolver::new())
        .expect("import default");
    let HirExpressionKind::Verte { source, target, .. } = imported.kind else {
        panic!("expected aggregate ascription");
    };
    assert_ne!(target, collision);
    assert_eq!(imported.ty, Some(target));
    assert_eq!(target_types.get(target), &Type::Array(string));
    assert_eq!(source.ty, Some(target));
    let HirExpressionKind::Array(items) = source.kind else {
        panic!("expected array");
    };
    let HirArrayElement::Expr(element) = &items[0] else {
        panic!("expected element");
    };
    let HirExpressionKind::Literal(HirLiteral::String(value)) = element.kind else {
        panic!("expected text literal");
    };
    assert_eq!(target_interner.resolve(value), "producer text");
}

#[test]
fn file_interface_default_rejects_unmapped_definition_references() {
    use crate::file_interface::InterfaceFieldDefault;
    use radix_hir::{DefId, HirExpression, HirExpressionKind, HirId};
    let expression = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(DefId(42)),
        ty: None,
        span: radix_lexer::Span::default(),
    };
    assert_eq!(
        InterfaceFieldDefault::snapshot(
            &expression,
            &TypeTable::new(),
            &Interner::new(),
            &Resolver::new(),
            None
        ),
        Err(FileInterfaceError::FieldDefaultUnsupported),
    );
}

#[test]
fn file_interface_round_trips_intersection_without_becoming_union() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let intersection = types.intern(Type::Intersection(vec![numerus, textus]));

    let snapshot =
        snapshot_interface_type(intersection, &types, &interner).expect("snapshot intersection");

    assert!(
        !matches!(snapshot, InterfaceTypeSnapshot::Union(_)),
        "an exported intersection must not snapshot as a union, got {snapshot:?}"
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import intersection");

    let Type::Intersection(members) = target_types.get(imported) else {
        panic!(
            "expected imported intersection, got {:?}",
            target_types.get(imported)
        );
    };
    assert_eq!(members.len(), 2);
    let kinds: Vec<_> = members
        .iter()
        .map(|member| target_types.get(*member).clone())
        .collect();
    assert!(kinds.contains(&Type::Primitive(Primitive::Numerus)));
    assert!(kinds.contains(&Type::Primitive(Primitive::Textus)));
}

/// Resolver holding one local `genus Foo` plus its semantic struct type, the
/// shape `@ radix type T mut Foo string` constrains a type parameter to.
fn resolver_with_struct_foo(
    types: &mut TypeTable,
    interner: &mut Interner,
) -> (Resolver, radix_hir::DefId, crate::TypeId) {
    let mut resolver = Resolver::new();
    let name = interner.intern("Foo");
    let def_id = resolver.fresh_def_id();
    let ty = types.intern(Type::Struct(def_id));
    resolver
        .define(crate::Symbol {
            def_id,
            name,
            kind: crate::SymbolKind::Struct,
            ty: Some(ty),
            mutable: false,
            span: radix_lexer::Span::default(),
        })
        .expect("Foo defines once");
    (resolver, def_id, ty)
}

/// A nominal in a type-param constraint snapshots through the same resolver the
/// parameter and return types use; the resolver-less path fails closed with
/// `NominalTypeUnsupported`, which skipped the whole export.
#[test]
fn callable_snapshot_keeps_nominal_type_param_constraint() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (resolver, _, foo) = resolver_with_struct_foo(&mut types, &mut interner);
    let textus = types.primitive(Primitive::Textus);
    let t = interner.intern("T");
    let sig = FuncSig {
        type_params: vec![GenericParamDescriptor {
            name: t,
            sort: GenericParamSort::Typus,
            constraint: crate::TypeParamConstraint::OneOf(vec![foo, textus]),
            default: None,
        }],
        params: Vec::new(),
        ret: textus,
        err: None,
        is_async: false,
        is_generator: false,
    };

    let callable = snapshot_interface_callable_with_resolver(&sig, &types, &interner, &resolver)
        .expect("a nominal constraint option must be representable");

    let InterfaceTypeParamConstraint::OneOf(options) = &callable.type_params[0].constraint else {
        panic!("expected a OneOf constraint");
    };
    assert_eq!(options.len(), 2);
    assert!(matches!(
        &options[0],
        InterfaceTypeSnapshot::Nominal(nominal)
            if nominal.name == "Foo" && nominal.kind == InterfaceNominalKind::Struct
    ));
    assert_eq!(
        options[1],
        InterfaceTypeSnapshot::Primitive(Primitive::Textus)
    );
}

/// The import leg resolves a nominal constraint option through the consumer's
/// resolver instead of failing closed.
#[test]
fn callable_import_resolves_nominal_type_param_constraint() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (resolver, _, foo) = resolver_with_struct_foo(&mut types, &mut interner);
    let callable = InterfaceCallable {
        type_params: vec![InterfaceCallableParam {
            name: "T".to_owned(),
            kind: GenericParamKind::Typus,
            constraint: InterfaceTypeParamConstraint::OneOf(vec![InterfaceTypeSnapshot::Nominal(
                InterfaceNominalType {
                    kind: InterfaceNominalKind::Struct,
                    name: "Foo".to_owned(),
                    identity: None,
                    kind_guessed: false,
                },
            )]),
            default: None,
        }],
        tensor_edge_policy_requirements: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
        err: None,
        is_async: false,
        is_generator: false,
        device: None,
    };

    let imported =
        import_interface_callable_with_resolver(&callable, &mut types, &mut interner, &resolver)
            .expect("a nominal constraint option must import");

    assert_eq!(
        imported.type_params[0].constraint,
        crate::TypeParamConstraint::OneOf(vec![foo])
    );
}

#[test]
fn sized_list_round_trips_through_the_snapshot() {
    // Need: `list<i8, size 32>` keeps its capacity across the module boundary.
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let elem = types.sized_numeric(Primitive::Numerus, NumericWidth::I8);
    let capacity = types.intern_index(IndexExpr::Literal(32));
    let sized = types.bounded_array(elem, capacity);

    let snapshot = snapshot_interface_type(sized, &types, &interner).expect("snapshot sized list");
    assert_eq!(
        snapshot,
        InterfaceTypeSnapshot::BoundedArray(
            Box::new(InterfaceTypeSnapshot::SizedNumeric(
                Primitive::Numerus,
                NumericWidth::I8
            )),
            InterfaceIndexSnapshot::Literal(32)
        )
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import sized list");
    let Type::BoundedArray { element, capacity } = target_types.get(imported) else {
        panic!(
            "expected imported bounded array, got {:?}",
            target_types.get(imported)
        );
    };
    assert!(matches!(
        target_types.get(*element),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I8)
    ));
    assert!(matches!(
        target_types.get_index(*capacity),
        IndexExpr::Literal(32)
    ));
}

#[test]
fn ranged_list_round_trips_through_the_snapshot() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let elem = types.sized_numeric(Primitive::Numerus, NumericWidth::I8);
    for (min, max) in [(2_u64, Some(5_u64)), (1, None)] {
        let ranged = types.ranged_array(elem, min, max);
        assert!(matches!(types.get(ranged), Type::RangedArray { .. }));

        let snapshot =
            snapshot_interface_type(ranged, &types, &interner).expect("snapshot ranged list");
        assert!(matches!(
            &snapshot,
            InterfaceTypeSnapshot::RangedArray { min: m, max: x, .. } if *m == min && *x == max
        ));

        let mut target_types = TypeTable::new();
        let mut target_interner = Interner::new();
        let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
            .expect("import ranged list");
        assert!(matches!(
            target_types.get(imported),
            Type::RangedArray { min: m, max: x, .. } if *m == min && *x == max
        ));
    }
}

#[test]
fn bounded_byte_list_round_trips_as_bounded_octeti() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let capacity = types.intern_index(IndexExpr::Literal(16));
    let octeti = types.bounded_octeti(capacity);

    let snapshot = snapshot_interface_type(octeti, &types, &interner).expect("snapshot octeti");
    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner)
        .expect("import octeti");
    assert!(matches!(
        target_types.get(imported),
        Type::BoundedOcteti { .. }
    ));
}
