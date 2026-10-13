//! Catch-binding error type when a local shadows an imported namespace.
//!
//! LCG2-CAPE-BIND-01: `shape.numel` inside `fac`/`cape` must constrain the
//! local error sink to the callee's declared carrier even when a parameter
//! named `shape` owns the Path DefId.

use std::collections::BTreeMap;

use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceEnumVariant,
    InterfaceNominalExport, InterfaceNominalKind, InterfaceNominalType, InterfaceParam,
    InterfaceTypeSnapshot,
};
use crate::{Primitive, SemanticParamMode, Type};
use radix_hir::visit::{HirVisitor, walk_cape, walk_program};

fn shape_namespace_interface() -> FileInterface {
    let shape_error = InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind: InterfaceNominalKind::Enum,
        name: "ShapeError".to_owned(),
        identity: None,
        kind_guessed: false,
    });
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "ShapeError".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "ShapeError".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![InterfaceEnumVariant {
                name: "Invalid".to_owned(),
                fields: Vec::new(),
            }],
        }),
    });
    interface.insert(FileExport {
        name: "numel".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: vec![InterfaceParam {
                ty: InterfaceTypeSnapshot::Array(Box::new(InterfaceTypeSnapshot::Primitive(
                    Primitive::Numerus,
                ))),
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
                axis: false,
            }],
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
            err: Some(Box::new(shape_error.clone())),
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        }),
    });
    interface.insert(FileExport {
        name: "message".to_owned(),
        kind: FileExportKind::Function(InterfaceCallable {
            type_params: Vec::new(),
            params: vec![InterfaceParam {
                ty: shape_error,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
                axis: false,
            }],
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        }),
    });
    interface
}

fn analyze_shape_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    let publica = crate::passes::visibility_annotation::VisibilityTier::Publica;
    crate::test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([(
            "shape".to_owned(),
            BTreeMap::from([
                ("ShapeError".to_owned(), publica),
                ("numel".to_owned(), publica),
                ("message".to_owned(), publica),
            ]),
        )]),
        BTreeMap::from([("shape".to_owned(), shape_namespace_interface())]),
    )
}

struct CapeBindingCollector {
    tys: Vec<Option<crate::TypeId>>,
}

impl HirVisitor for CapeBindingCollector {
    fn visit_cape(&mut self, cape: &radix_hir::HirCape) {
        self.tys.push(cape.binding_ty);
        walk_cape(self, cape);
    }
}

fn cape_binding_tys(unit: &crate::test_support::AnalyzedFixture) -> Vec<Option<crate::TypeId>> {
    let mut collector = CapeBindingCollector { tys: Vec::new() };
    walk_program(&mut collector, unit.hir.as_ref().expect("hir"));
    collector.tys
}

#[test]
fn imported_namespace_failable_under_local_shadow_binds_catch_error_type() {
    // Shadowed twin: parameter `shape` owns the Path DefId. Unshadowed twin:
    // parameter `forma` leaves the imported namespace visible. Both handled
    // bodies call the same failable `shape.numel`; both catch bindings must
    // resolve the foreign `ShapeError`, not ignotum / the enclosing TensorError.
    let source = r#"importa ex "./shape" shape

discretio TensorError {
    Invalidum { textus nuntius }
}

functio _numel_shape(lista<numerus> shape) → numerus ⇥ TensorError {
    fac {
        redde shape.numel(shape)
    }
    cape err {
        redde 0
    }
}

functio _numel_forma(lista<numerus> forma) → numerus ⇥ TensorError {
    fac {
        redde shape.numel(forma)
    }
    cape err {
        redde 0
    }
}"#;
    let mut unit = analyze_shape_consumer(source);
    assert!(
        unit.is_ok(),
        "shadowed and unshadowed namespace wrappers should analyze green: {:?}",
        unit.diagnostics
    );
    let errors: Vec<_> = unit.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected no typecheck errors, got: {errors:?}"
    );

    let shape = unit.interner.intern("shape");
    let shape_error_name = unit.interner.intern("ShapeError");
    let numel_name = unit.interner.intern("numel");
    let tensor_error_name = unit.interner.intern("TensorError");
    let foreign_err = unit
        .resolver
        .namespace_file_type(shape, shape_error_name)
        .expect("imported ShapeError type");
    let numel = unit
        .resolver
        .namespace_file_function_canonical(shape, numel_name)
        .expect("imported numel signature");
    let callee_err = numel.err.expect("numel is failable");
    assert!(
        unit.types.equals(callee_err, foreign_err),
        "numel's declared carrier must be the imported ShapeError"
    );

    let tensor_error_def = unit
        .resolver
        .lookup(tensor_error_name)
        .expect("local TensorError");
    let enclosing_err = unit.types.intern(Type::Enum(tensor_error_def));
    let ignotum = unit.types.primitive(Primitive::Ignotum);
    assert!(!unit.types.equals(foreign_err, enclosing_err));
    assert!(!unit.types.equals(foreign_err, ignotum));

    let bindings = cape_binding_tys(&unit);
    let name_of = |def_id| {
        unit.resolver.get_symbol(def_id).map_or_else(
            || format!("{def_id:?}"),
            |symbol| unit.interner.resolve(symbol.name).to_owned(),
        )
    };
    let show = |ty| crate::format_faber_type(ty, &unit.types, &unit.interner, &name_of);
    assert_eq!(
        bindings.len(),
        2,
        "expected one catch each on the shadowed and unshadowed wrappers, got {bindings:?}"
    );
    for binding in bindings {
        let binding = binding.expect("catch binding_ty must be recorded");
        assert!(
            unit.types.equals(binding, foreign_err),
            "catch binding must be the callee ShapeError, not ignotum/enclosing; got {} (foreign {} enclosing {} ignotum {})",
            show(binding),
            show(foreign_err),
            show(enclosing_err),
            show(ignotum)
        );
        assert!(
            !unit.types.equals(binding, ignotum),
            "catch binding must not fall back to ignotum"
        );
        assert!(
            !unit.types.equals(binding, enclosing_err),
            "catch binding must not fall back to the enclosing TensorError channel"
        );
    }
}

/// KPC-SEM boundary control: a local that shadows an import name owns the
/// Path DefId, so with no namespace surface installed at all the receiver is
/// typed as the local value — the bare `lista<numerus>` method surface, not a
/// namespace member route. Namespace-call repairs must key on installed
/// surfaces, never on the binding name alone.
#[test]
fn shadowed_namespace_name_without_surface_types_as_local_value() {
    // (The shadowing name is `cura`, not `nulla`: D23.4 revived `nulla` as the
    // null value literal, so it is no longer an ordinary identifier.)
    let unit = crate::test_support::analyze_source_with_import_contract(
        r#"importa ex "./cura" cura

functio _probe(lista<numerus> cura) → numerus {
    redde cura.longitudo()
}"#,
        BTreeMap::new(),
        BTreeMap::new(),
    );
    assert!(
        unit.is_ok(),
        "a shadowing parameter keeps value typing without any import surface: {:?}",
        unit.diagnostics
    );
}
