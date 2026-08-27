//! S0-R1: interface snapshots serialize kinded applied size arguments.

use crate::file_interface::{
    import_interface_callable_with_resolver, import_interface_type,
    snapshot_interface_callable_with_resolver, snapshot_interface_type, FileInterfaceError,
    InterfaceAppliedArg, InterfaceIndexSnapshot, InterfaceTypeSnapshot,
};
use crate::{
    FuncSig, IndexExpr, InferVar, NumericWidth, ParamType, Primitive, Resolver, Type, TypeTable,
};
use radix_lexer::Interner;
use radix_types::AppliedArg;

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
        type_params: vec![m, k],
        type_param_constraints: vec![
            crate::TypeParamConstraint::Any,
            crate::TypeParamConstraint::Any,
        ],
        params: vec![ParamType {
            ty: tensor,
            mode: crate::SemanticParamMode::Owned,
            optional: false,
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

    assert_eq!(callable.type_params, vec!["M".to_owned(), "K".to_owned()]);
    assert_eq!(callable.type_param_constraints.len(), 2);
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
        .map(|param| target_interner.resolve(*param).to_owned())
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

/// Fail-closed pins (PKG-REP done_when (d)): inference holes, error types,
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

    // A nominal without a resolver stays unsupported rather than guessed.
    let nominal = types.intern(Type::Struct(radix_hir::DefId(1)));
    assert_eq!(
        snapshot_interface_type(nominal, &types, &interner),
        Err(FileInterfaceError::NominalTypeUnsupported {
            kind: crate::file_interface::InterfaceNominalKind::Struct
        })
    );
}
