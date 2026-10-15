//! A static (`generis`) genus field reads through the type name on the runner:
//! the read is the field's folded initializer, with no instance and no struct
//! field projection.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const TAB: &str =
    "genus Tab {\n    generis numerus Z = 3\n    generis textus NOMEN = \"tabula\"\n    varia numerus w = 1\n}\n";

fn run(body: &str) -> Result<Vec<String>, String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("{TAB}incipit {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session, "static_field_read.fab", &source, &mut host) {
        Ok(()) => Ok(host.stdout_lines),
        Err(RunSourceError::Stepper(errors)) => Err(format!("{errors:?}")),
        Err(other) => Err(format!("{other:?}")),
    }
}

#[test]
fn static_field_read_through_type_name_prints_the_initializer() {
    let lines = run("nota Tab.Z\nnota Tab.NOMEN").expect("static reads run");
    assert_eq!(lines, vec!["3", "tabula"]);
}

#[test]
fn static_field_read_in_an_expression_and_beside_an_instance() {
    let lines = run("sit Tab t ← Tab {}\nsit numerus n ← Tab.Z + t.w\nnota n").expect("run");
    assert_eq!(lines, vec!["4"]);
}

#[test]
fn static_field_read_through_an_instance_is_a_compile_error_not_invalid_mir() {
    let err = run("sit Tab t ← Tab {}\nnota t.Z").expect_err("instance read of a static");
    assert!(!err.contains("invalid MIR"), "{err}");
    assert!(err.contains("static_field_read_through_instance"), "{err}");
}

#[test]
fn static_field_read_of_an_instance_field_through_the_type_is_a_compile_error() {
    let err = run("nota Tab.w").expect_err("type read of an instance field");
    assert!(!err.contains("invalid MIR"), "{err}");
    assert!(err.contains("instance_field_read_through_type"), "{err}");
}

#[test]
fn static_field_read_through_an_imported_genus_inlines_the_interface_initializer() {
    use crate::driver::{AnalyzeRequest, ModuleImportContract};
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceStructExport, InterfaceStructField, InterfaceTypeSnapshot,
    };
    use crate::hir::{HirExpression, HirExpressionKind, HirId, HirLiteral};
    use crate::semantic::Primitive;
    use crate::syntax::{FieldModifier, MemberVisibility};

    let init = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Literal(HirLiteral::Int(4242)),
        ty: None,
        span: radix_lexer::Span::default(),
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Tab".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Tab".to_owned(),
            fields: vec![InterfaceStructField {
                name: "Z".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: false,
                required: false,
                default: Some(
                    crate::file_interface::InterfaceFieldDefault::snapshot(
                        &init,
                        &crate::semantic::TypeTable::new(),
                        &crate::lexer::Interner::new(),
                        &crate::semantic::Resolver::new(),
                        None,
                    )
                    .expect("portable scalar default"),
                ),
                modifier: Some(FieldModifier::Generis),
                visibility: MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = crate::driver::analyze_source(
        &session,
        "test.fab",
        "importa ex \"demo:lib\" demo\nfunctio g() → numerus { redde demo.Tab.Z }\n",
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports: std::collections::BTreeMap::new(),
                file_interfaces: std::collections::BTreeMap::from([("demo".to_owned(), interface)]),
                ..ModuleImportContract::default()
            },
        },
    )
    .expect("imported static read analyzes");
    let dump = crate::mir::dump_analyzed_unit(unit.application_view()).expect("imported static read lowers");
    assert!(dump.contains("4242"), "expected the inlined initializer, got:\n{dump}");
}

#[test]
fn imported_module_constant_read_and_index_lower_to_the_interface_value() {
    use crate::driver::{AnalyzeRequest, ModuleImportContract};
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceConstValue, InterfaceTypeSnapshot,
    };
    use crate::semantic::Primitive;
    use crate::semantic::passes::visibility_annotation::VisibilityTier;
    use std::collections::BTreeMap;

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "BASE".to_owned(),
        kind: FileExportKind::Const(
            InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
            Some(InterfaceConstValue::Int(5)),
        ),
    });
    let contract = ModuleImportContract {
        namespace_exports: BTreeMap::from([(
            "constantia".to_owned(),
            BTreeMap::from([("BASE".to_owned(), VisibilityTier::Publica)]),
        )]),
        file_interfaces: BTreeMap::from([("constantia".to_owned(), interface)]),
        ..ModuleImportContract::default()
    };
    let session = Session::new(Config::default().with_dev_locale_root());
    let source = "importa ex \"./constantia\" constantia\n\
        functio pick(lista<numerus> xs) → numerus { redde xs[constantia.BASE] }\n";
    let unit = crate::driver::analyze_source(
        &session,
        "imported_constant_index.fab",
        source,
        AnalyzeRequest { cli_program: None, contract },
    )
    .expect("imported constant index analyzes");
    let dump = crate::mir::dump_analyzed_unit(unit.application_view())
        .expect("imported constant index lowers in single-file MIR");
    assert!(dump.contains("5"), "expected the inlined constant, got:\n{dump}");
}

#[test]
fn imported_list_constant_index_lowers_to_the_interface_list() {
    use crate::driver::{AnalyzeRequest, ModuleImportContract};
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceConstValue, InterfaceTypeSnapshot,
    };
    use crate::semantic::Primitive;
    use crate::semantic::passes::visibility_annotation::VisibilityTier;
    use std::collections::BTreeMap;

    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "TABLE".to_owned(),
        kind: FileExportKind::Const(
            InterfaceTypeSnapshot::Array(Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus))),
            Some(InterfaceConstValue::List(vec![
                InterfaceConstValue::Int(1111),
                InterfaceConstValue::Int(2222),
                InterfaceConstValue::Int(3333),
            ])),
        ),
    });
    let contract = ModuleImportContract {
        namespace_exports: BTreeMap::from([(
            "constantia".to_owned(),
            BTreeMap::from([("TABLE".to_owned(), VisibilityTier::Publica)]),
        )]),
        file_interfaces: BTreeMap::from([("constantia".to_owned(), interface)]),
        ..ModuleImportContract::default()
    };
    let session = Session::new(Config::default().with_dev_locale_root());
    let source = "importa ex \"./constantia\" constantia\n\
        functio pick(numerus i) → numerus { redde constantia.TABLE[i] }\n";
    let unit = crate::driver::analyze_source(
        &session,
        "imported_list_constant_index.fab",
        source,
        AnalyzeRequest { cli_program: None, contract },
    )
    .expect("imported list constant index analyzes");
    let dump = crate::mir::dump_analyzed_unit(unit.application_view())
        .expect("imported list constant index lowers in single-file MIR");
    for value in ["1111", "2222", "3333"] {
        assert!(dump.contains(value), "expected the inlined list element {value}, got:\n{dump}");
    }
}
