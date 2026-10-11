//! Tests for file interface type introspection and snapshot.
//! Proves: interface import/snapshot against radix-hir-rust bridges;
//! nominal kinds, type params, and callables resolve correctly.

use super::{
    FileExport, FileExportKind, FileInterface, FileInterfaceError, InterfaceAliasExport, InterfaceAppliedArg,
    InterfaceBodyChannel, InterfaceCallable, InterfaceCallableParam, InterfaceCanonicalBody, InterfaceDeviceContract,
    InterfaceDeviceRole, InterfaceDeviceVisibility, InterfaceGenericParam, InterfaceIndexSnapshot,
    InterfaceLibraryIdentity, InterfaceNominalKind, InterfaceNominalType, InterfaceParam, InterfaceStructExport,
    InterfaceStructField, InterfaceTypeParamConstraint, InterfaceTypeSnapshot, import_interface_type,
    snapshot_interface_type,
};
use crate::lexer::Interner;
use crate::semantic::{
    GenericParamDescriptor, GenericParamSort, IndexExpr, InstansPrecision, NumericWidth, ParamType, Primitive,
    SemanticGenericParamDefault, SemanticMutability, SemanticParamMode, Type, TypeParamConstraint, TypeTable,
};
use std::collections::BTreeMap;

#[test]
fn primitive_snapshot_imports_to_target_primitive_id() {
    let source_types = TypeTable::new();
    let source_interner = Interner::new();
    let snapshot = snapshot_interface_type(source_types.primitive(Primitive::Textus), &source_types, &source_interner)
        .expect("snapshot");

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner).expect("import");

    assert_eq!(imported, target_types.primitive(Primitive::Textus));
}

#[test]
fn tuple_snapshot_imports_to_target_tuple_type() {
    // Companion signatures return a gradient tuple; the interface model must
    // snapshot and import `Type::Tuple` so companion exports survive the
    // cross-unit boundary (SEM004).
    let mut source_types = TypeTable::new();
    let source_interner = Interner::new();
    let textus = source_types.primitive(Primitive::Textus);
    let numerus = source_types.primitive(Primitive::Numerus);
    let tuple_ty = source_types.tuple(vec![textus, numerus]);

    let snapshot = snapshot_interface_type(tuple_ty, &source_types, &source_interner).expect("snapshot");
    assert!(
        matches!(snapshot, InterfaceTypeSnapshot::Tuple(_)),
        "expected tuple snapshot, got {snapshot:?}"
    );

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner).expect("import");

    let Type::Tuple(elements) = target_types.get(imported) else {
        panic!("expected tuple type");
    };
    assert_eq!(elements.as_slice(), &[textus, numerus]);
}

#[test]
fn failable_generic_callable_round_trips() {
    let mut source_types = TypeTable::new();
    let mut source_interner = Interner::new();
    let param_t = source_interner.intern("T");
    let textus = source_types.primitive(Primitive::Textus);
    let numerus = source_types.primitive(Primitive::Numerus);
    let param_ty = source_types.intern(Type::Param(param_t));
    let lista_t = source_types.array(param_ty);
    let sig = crate::semantic::FuncSig {
        type_params: vec![GenericParamDescriptor {
            name: param_t,
            sort: GenericParamSort::Typus,
            constraint: TypeParamConstraint::OneOf(vec![textus, numerus]),
            default: Some(SemanticGenericParamDefault::Typus(numerus)),
        }],
        params: vec![ParamType { ty: lista_t, mode: SemanticParamMode::Owned, optional: true, sponte: true }],
        ret: param_ty,
        err: Some(textus),
        is_async: true,
        is_generator: false,
    };
    let func_ty = source_types.intern(Type::Func(sig));

    let snapshot = snapshot_interface_type(func_ty, &source_types, &source_interner).expect("snapshot");

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner).expect("import");

    let Type::Func(imported_sig) = target_types.get(imported) else {
        panic!("expected imported function type");
    };
    assert_eq!(imported_sig.type_params.len(), 1);
    assert_eq!(target_interner.resolve(imported_sig.type_params[0].name), "T");
    assert_eq!(imported_sig.type_params[0].sort, GenericParamSort::Typus);
    assert_eq!(imported_sig.params.len(), 1);
    assert!(imported_sig.params[0].optional);
    assert_eq!(imported_sig.params[0].mode, SemanticParamMode::Owned);
    assert_eq!(imported_sig.err, None);
    let Type::PromissumFailable(success, alternate) = target_types.get(imported_sig.ret) else {
        panic!("expected imported async failable return carrier");
    };
    assert!(matches!(target_types.get(*success), Type::Param(sym) if target_interner.resolve(*sym) == "T"));
    assert_eq!(*alternate, target_types.primitive(Primitive::Textus));
    assert!(imported_sig.is_async);
    assert!(!imported_sig.is_generator);

    let TypeParamConstraint::OneOf(options) = &imported_sig.type_params[0].constraint else {
        panic!("expected type param constraint");
    };
    assert_eq!(
        options,
        &vec![
            target_types.primitive(Primitive::Textus),
            target_types.primitive(Primitive::Numerus)
        ]
    );
    let Some(SemanticGenericParamDefault::Typus(default_ty)) = imported_sig.type_params[0].default else {
        panic!("expected typus default to survive the snapshot round trip");
    };
    assert_eq!(default_ty, target_types.primitive(Primitive::Numerus));
}

#[test]
fn structural_collection_and_index_shapes_round_trip() {
    let mut source_types = TypeTable::new();
    let mut source_interner = Interner::new();
    let n = source_interner.intern("N");
    let textus = source_types.primitive(Primitive::Textus);
    let dim2 = source_types.intern_index(IndexExpr::Literal(2));
    let dim_n = source_types.intern_index(IndexExpr::Param(n));
    let shape = source_types.intern_index(IndexExpr::Tuple(vec![dim2, dim_n]));
    let tensor = source_types.tensor_with_shape(textus, shape);
    let option = source_types.option(tensor);
    let reference = source_types.reference(SemanticMutability::Immutable, option);

    let snapshot = snapshot_interface_type(reference, &source_types, &source_interner).expect("snapshot");

    let mut target_types = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target_types, &mut target_interner).expect("import");

    let Type::Ref(SemanticMutability::Immutable, option_id) = target_types.get(imported) else {
        panic!("expected ref type");
    };
    let Type::Option(tensor_id) = target_types.get(*option_id) else {
        panic!("expected option type");
    };
    let Type::Tensor(element, shape_id) = target_types.get(*tensor_id) else {
        panic!("expected tensor type");
    };
    assert_eq!(*element, target_types.primitive(Primitive::Textus));

    let IndexExpr::Tuple(dims) = target_types.get_index(*shape_id) else {
        panic!("expected tuple shape");
    };
    assert!(matches!(target_types.get_index(dims[0]), IndexExpr::Literal(2)));
    assert!(matches!(target_types.get_index(dims[1]), IndexExpr::Param(sym) if target_interner.resolve(*sym) == "N"));
}

#[test]
fn manual_snapshot_imports_records_unions_and_sized_types() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "value".to_owned(),
        InterfaceTypeSnapshot::Union(vec![
            InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
            InterfaceTypeSnapshot::SizedInstans(InstansPrecision::Millis),
        ]),
    );
    let snapshot = InterfaceTypeSnapshot::Record(fields);

    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut types, &mut interner).expect("import");

    let Type::Record(fields) = types.get(imported) else {
        panic!("expected record");
    };
    let value_sym = interner.intern("value");
    let value_ty = fields.get(&value_sym).expect("value field");
    let Type::Union(members) = types.get(*value_ty) else {
        panic!("expected union");
    };
    assert!(matches!(
        types.get(members[0]),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
    ));
    assert!(matches!(types.get(members[1]), Type::SizedInstans(InstansPrecision::Millis)));
}

#[test]
fn callable_snapshot_can_be_constructed_without_source_type_table() {
    let callable = InterfaceCallable {
        type_params: vec![InterfaceCallableParam {
            name: "T".to_owned(),
            kind: crate::syntax::GenericParamKind::Typus,
            constraint: InterfaceTypeParamConstraint::Any,
            default: None,
        }],
        params: vec![InterfaceParam {
            ty: InterfaceTypeSnapshot::Param("T".to_owned()),
            mode: SemanticParamMode::Move,
            optional: false,
            sponte: false,
        }],
        ret: Box::new(InterfaceTypeSnapshot::Param("T".to_owned())),
        err: Some(Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus))),
        is_async: false,
        is_generator: true,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    };
    let snapshot = InterfaceTypeSnapshot::Func(callable);
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(&snapshot, &mut types, &mut interner).expect("import");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected callable");
    };
    assert!(sig.is_generator);
    assert_eq!(sig.err, Some(types.primitive(Primitive::Textus)));
}

#[test]
fn async_callable_import_wraps_declared_return_in_promissum() {
    let callable = InterfaceCallable {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
        err: None,
        is_async: true,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    };
    let snapshot = InterfaceTypeSnapshot::Func(callable);
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(&snapshot, &mut types, &mut interner).expect("import");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected callable");
    };
    let Type::Promissum(inner) = types.get(sig.ret) else {
        panic!("expected async callable return to be promissum");
    };
    assert_eq!(*inner, types.primitive(Primitive::Vacuum));
}

#[test]
fn async_generator_callable_import_wraps_declared_return_in_async_cursor() {
    let callable = InterfaceCallable {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
        err: Some(Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus))),
        is_async: true,
        is_generator: true,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    };
    let snapshot = InterfaceTypeSnapshot::Func(callable);
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(&snapshot, &mut types, &mut interner).expect("import");

    let Type::Func(sig) = types.get(imported) else {
        panic!("expected callable");
    };
    let Type::AsyncCursor(item, alternate) = types.get(sig.ret) else {
        panic!("expected async generator callable return to be async cursor");
    };
    assert_eq!(*item, types.primitive(Primitive::Numerus));
    assert_eq!(*alternate, types.primitive(Primitive::Textus));
    assert_eq!(sig.err, None);
    assert!(sig.is_async);
    assert!(sig.is_generator);
}

#[test]
fn nominal_type_import_fails_closed() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let snapshot = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Struct,
        name: "Terminus".to_owned(),
        identity: None,
        kind_guessed: false,
    });

    let err = import_interface_type(&snapshot, &mut types, &mut interner).expect_err("nominal unsupported");

    assert_eq!(
        err,
        FileInterfaceError::NominalTypeUnsupported { kind: InterfaceNominalKind::Struct }
    );
}

#[test]
fn index_inference_snapshot_fails_closed() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let shape = types.intern_index(IndexExpr::Infer(crate::semantic::IndexVar(0)));
    let textus = types.primitive(Primitive::Textus);
    let tensor = types.tensor_with_shape(textus, shape);

    let err = snapshot_interface_type(tensor, &types, &interner).expect_err("index inference unsupported");

    assert_eq!(err, FileInterfaceError::IndexInferenceUnsupported);
}

#[test]
fn index_snapshot_imports_unspecified_shape() {
    let snapshot = InterfaceTypeSnapshot::Tensor(
        Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
        InterfaceIndexSnapshot::Unspecified,
    );
    let mut types = TypeTable::new();
    let mut interner = Interner::new();

    let imported = import_interface_type(&snapshot, &mut types, &mut interner).expect("import");

    let Type::Tensor(_, shape) = types.get(imported) else {
        panic!("expected tensor");
    };
    assert!(matches!(types.get_index(*shape), IndexExpr::Unspecified));
}

#[test]
fn conflicting_kind_snapshot_fails_closed_through_canonical_key() {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity, InterfaceStructExport,
        InterfaceStructField, import_interface_type_with_resolver,
    };
    use crate::semantic::Resolver;

    let identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    let mut interface = FileInterface::new().with_identity(identity.clone());
    interface.insert(FileExport {
        name: "Terminus".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            params: Vec::new(),
            name: "Terminus".to_owned(),
            fields: vec![InterfaceStructField {
                name: "gradus".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: crate::syntax::MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("utilModule".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("install struct interface");

    // A snapshot that claims the resolved symbol is an enum (it is a struct)
    // must fail closed through the canonical key — the identity-bearing
    // snapshot's canonical key (Enum + identity + name) is unknown to the
    // consumer, and the unknown-identity error fires instead of any silent
    // fall-through to a name-keyed or local-symbol resolution.
    let snapshot = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Enum,
        name: "Terminus".to_owned(),
        identity: Some(identity),
        kind_guessed: false,
    });
    let err = import_interface_type_with_resolver(&snapshot, &mut types, &mut interner, &resolver)
        .expect_err("conflicting kind must fail closed");
    assert_eq!(
        err,
        FileInterfaceError::NominalIdentityUnknown { kind: InterfaceNominalKind::Enum, name: "Terminus".to_owned() }
    );
}

/// Fail-closed (council-11 correct_before_next_phase): an identity-bearing
/// nominal snapshot whose canonical key the consumer's canonical table does
/// not know must return an explicit unknown-identity error — NOT fall through
/// to legacy short-name / local-symbol resolution, which could silently bind
/// the unknown nominal to a DIFFERENT consumer nominal.
#[test]
fn identity_bearing_snapshot_canonical_miss_fails_closed() {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity, InterfaceStructExport,
        import_interface_type_with_resolver,
    };
    use crate::semantic::Resolver;

    let identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    // The consumer installed a DIFFERENT module's interface: a same-name
    // `Persona` exists, but under the `other` module identity — the
    // identity-bearing key `util::Persona` is absent. Legacy name-based
    // fallback would silently bind the snapshot to this unrelated nominal.
    let other = FileInterface::new().with_identity(InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["other".to_owned()],
    });
    let other = {
        let mut i = other;
        i.insert(FileExport {
            name: "Persona".to_owned(),
            kind: FileExportKind::Struct(InterfaceStructExport {
                is_json: false,
                params: Vec::new(),
                name: "Persona".to_owned(),
                fields: Vec::new(),
                methods: Vec::new(),
                annotation_contract: None,
            }),
        });
        i
    };

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("otherModule".to_owned(), other)]),
            &mut interner,
            &mut types,
        )
        .expect("install interface");

    let snapshot = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Struct,
        name: "Persona".to_owned(),
        identity: Some(identity),
        kind_guessed: false,
    });
    let err = import_interface_type_with_resolver(&snapshot, &mut types, &mut interner, &resolver)
        .expect_err("canonical miss on identity-bearing snapshot must fail closed");
    assert_eq!(
        err,
        FileInterfaceError::NominalIdentityUnknown { kind: InterfaceNominalKind::Struct, name: "Persona".to_owned() }
    );
}

#[test]
fn reexported_same_module_nominal_callable_remaps_through_parent_interface() {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity, InterfaceStructExport,
    };
    use crate::semantic::Resolver;

    let parent_identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("triga".to_owned()),
        module_path: vec!["geometry".to_owned()],
    };
    let child_identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("triga".to_owned()),
        module_path: vec!["geometry".to_owned(), "data".to_owned()],
    };
    let mut interface = FileInterface::new().with_identity(parent_identity);
    interface.insert(FileExport {
        name: "data.BufferGeometry".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            params: Vec::new(),
            name: "BufferGeometry".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    interface.insert(FileExport {
        name: "data.geometry_vertex_layout_matches".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: vec![InterfaceParam {
                ty: InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
                    kind: InterfaceNominalKind::Struct,
                    name: "BufferGeometry".to_owned(),
                    identity: Some(child_identity),
                    kind_guessed: false,
                }),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            }],
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Bivalens)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let skipped = resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("geometry".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("install re-exported interface");

    assert!(
        skipped.is_empty(),
        "same-module nominal must not poison the callable: {skipped:?}"
    );
    assert!(
        resolver
            .namespace_file_function(
                interner.intern("geometry"),
                interner.intern("data.geometry_vertex_layout_matches"),
            )
            .is_some()
    );
}

#[test]
fn strict_locale_pack_keeps_supported_struct_fields_without_field_rows() {
    use crate::locale::LocalePack;
    use crate::semantic::Resolver;

    let identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("triga".to_owned()),
        module_path: vec!["geometry".to_owned(), "attribute".to_owned()],
    };
    let unknown = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Struct,
        name: "Missing".to_owned(),
        identity: Some(InterfaceLibraryIdentity {
            provider: "package".to_owned(),
            package: Some("other".to_owned()),
            module_path: vec!["geometry".to_owned()],
        }),
        kind_guessed: false,
    });
    let mut interface = FileInterface::new().with_identity(identity.clone());
    interface.insert(FileExport {
        name: "BufferAttribute".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            params: Vec::new(),
            name: "BufferAttribute".to_owned(),
            fields: vec![
                InterfaceStructField {
                    name: "name".to_owned(),
                    ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: crate::syntax::MemberVisibility::Publica,
                },
                InterfaceStructField {
                    name: "unsupported".to_owned(),
                    ty: unknown,
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: crate::syntax::MemberVisibility::Publica,
                },
            ],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "en-test"
schema_version = 1
fallback = ["la"]

[[library_members]]
provider = "package"
package = "triga"
module_path = ["geometry", "attribute"]
kind = "type"
canonical = "BufferAttribute"
surface = "BufferAttribute"

[llm]
system_prompt_snippet = "Emit test source"
exemplars = ["test.fab"]
"#,
    )
    .expect("library-member locale pack");

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let skipped = resolver
        .set_file_interfaces_from_snapshots_with_locale(
            &BTreeMap::from([("attribute".to_owned(), interface)]),
            &mut interner,
            &mut types,
            Some(&pack),
        )
        .expect("install struct interface");

    let fields = resolver
        .imported_file_structs_for_namespace(interner.intern("attribute"))
        .into_iter()
        .next()
        .expect("BufferAttribute")
        .1;
    assert_eq!(fields.len(), 1);
    assert_eq!(interner.resolve(fields[0].name), "name");
    let skipped_field = skipped
        .iter()
        .find(|skip| skip.canonical_name == "unsupported")
        .expect("unsupported field should retain WARN014 provenance");
    assert!(
        skipped_field
            .remap_reason
            .contains("NominalIdentityUnknown")
    );
}

/// S0-R1: declaration parameter kind/order rides the portable schema and the
/// consumer resolver registers the ordered kinds for imported declarations,
/// so `module.Type<…>` applications are kind-directed and mismatches fail
/// closed instead of accepting any argument spelling.
#[test]
fn file_interface_preserves_generic_size_decl_kinds_on_import() {
    use crate::semantic::Resolver;
    use crate::syntax::GenericParamKind;

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Vector".to_owned(),
        kind: FileExportKind::TypeAlias(InterfaceAliasExport {
            name: "Vector".to_owned(),
            params: vec![InterfaceGenericParam { name: "N".to_owned(), kind: GenericParamKind::Magnitudo }],
            body: InterfaceTypeSnapshot::Vector(
                Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32)),
                InterfaceIndexSnapshot::Param("N".to_owned()),
            ),
        }),
    });
    interface.insert(FileExport {
        name: "Box".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            params: vec![InterfaceGenericParam { name: "N".to_owned(), kind: GenericParamKind::Magnitudo }],
            name: "Box".to_owned(),
            fields: Vec::new(),
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });

    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    resolver
        .set_file_interfaces_from_snapshots(
            &BTreeMap::from([("math".to_owned(), interface)]),
            &mut interner,
            &mut types,
        )
        .expect("install interface");

    assert_eq!(
        resolver.namespace_file_decl_param_kinds(interner.intern("math"), interner.intern("Vector")),
        Some(vec![GenericParamKind::Magnitudo])
    );
    assert_eq!(
        resolver.namespace_file_decl_param_kinds(interner.intern("math"), interner.intern("Box")),
        Some(vec![GenericParamKind::Magnitudo])
    );
    assert_eq!(
        resolver.namespace_file_decl_param_kinds(interner.intern("math"), interner.intern("missing")),
        None
    );
}

/// S0-R1: a size-bearing applied snapshot round-trips through the re-exported
/// module surface; the size argument stays a kinded index, never a type.
#[test]
fn file_interface_preserves_generic_size_applied_round_trip() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = types.intern_index(IndexExpr::Literal(3));
    let base = types.vector_with_width(elem, width);
    let applied = types.intern(Type::Applied(base, vec![radix_types::AppliedArg::Size(width)]));

    let snapshot = snapshot_interface_type(applied, &types, &interner).expect("snapshot applied size");
    assert_eq!(
        snapshot,
        InterfaceTypeSnapshot::Applied(
            Box::new(InterfaceTypeSnapshot::Vector(
                Box::new(InterfaceTypeSnapshot::SizedNumeric(Primitive::Fractus, NumericWidth::F32)),
                InterfaceIndexSnapshot::Literal(3),
            )),
            vec![InterfaceAppliedArg::Size(InterfaceIndexSnapshot::Literal(3))],
        )
    );

    let mut target = TypeTable::new();
    let mut target_interner = Interner::new();
    let imported = import_interface_type(&snapshot, &mut target, &mut target_interner).expect("import applied size");
    let Type::Applied(_, args) = target.get(imported) else {
        panic!("expected imported applied type");
    };
    assert!(matches!(
        args.as_slice(),
        [radix_types::AppliedArg::Size(index)] if matches!(target.get_index(*index), IndexExpr::Literal(3))
    ));
}

// DFV2-3 imported role + body channel: the transport types distinguish the
// three body states and never treat a missing body as composable.

/// DFV2-3 (c): the body channel explicitly distinguishes a source body, a
/// serialized canonical artifact, and a missing/incompatible body.
#[test]
fn device_body_channel_distinguishes_source_canonical_and_missing() {
    let source = InterfaceDeviceContract {
        role: InterfaceDeviceRole::Kernel,
        visibility: InterfaceDeviceVisibility::Public,
        body: InterfaceBodyChannel::Source,
    };
    let canonical = InterfaceDeviceContract {
        role: InterfaceDeviceRole::Kernel,
        visibility: InterfaceDeviceVisibility::Public,
        body: InterfaceBodyChannel::Canonical(InterfaceCanonicalBody::new(b"template".to_vec())),
    };
    let missing = InterfaceDeviceContract {
        role: InterfaceDeviceRole::Kernel,
        visibility: InterfaceDeviceVisibility::Public,
        body: InterfaceBodyChannel::Missing,
    };

    assert_ne!(source.body, canonical.body);
    assert_ne!(canonical.body, missing.body);
    assert_ne!(source.body, missing.body);
    assert!(source.is_composable(), "a source body is composable");
    assert!(canonical.is_composable(), "a canonical artifact is composable");
    assert!(!missing.is_composable(), "a missing body is never composable");
}

/// DFV2-3 (d): the canonical body hash is deterministic for identical bytes
/// and distinguishes different bodies — the cache-identity input for DFV2-5.
#[test]
fn canonical_body_hash_is_deterministic_and_discriminating() {
    let first = InterfaceCanonicalBody::new(b"body-a".to_vec());
    let same = InterfaceCanonicalBody::new(b"body-a".to_vec());
    let other = InterfaceCanonicalBody::new(b"body-b".to_vec());

    assert_eq!(first.body_hash, same.body_hash);
    assert_ne!(first.body_hash, other.body_hash);
    assert_eq!(first.template, b"body-a".to_vec());
}

/// DFV2-3 (a): role and visibility are independent facts on the contract —
/// a private helper keeps the Kernel role without the launch visibility.
#[test]
fn device_contract_keeps_role_and_visibility_independent() {
    let helper = InterfaceDeviceContract {
        role: InterfaceDeviceRole::Kernel,
        visibility: InterfaceDeviceVisibility::Private,
        body: InterfaceBodyChannel::Source,
    };
    let fragment = InterfaceDeviceContract {
        role: InterfaceDeviceRole::Fragment,
        visibility: InterfaceDeviceVisibility::Public,
        body: InterfaceBodyChannel::Source,
    };

    assert_ne!(helper, fragment);
    assert_eq!(helper.role, InterfaceDeviceRole::Kernel);
    assert_eq!(helper.visibility, InterfaceDeviceVisibility::Private);
    assert_eq!(fragment.role, InterfaceDeviceRole::Fragment);
    assert_eq!(fragment.visibility, InterfaceDeviceVisibility::Public);
}
