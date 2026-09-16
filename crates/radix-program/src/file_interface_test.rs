use super::*;
use radix_module::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};
use radix_module::file_interface::{
    FileExportKind, InterfaceBodyChannel, InterfaceDeviceRole, InterfaceDeviceVisibility,
    InterfaceStructField, InterfaceTypeSnapshot,
};
use radix_module::semantic::Primitive;
use std::collections::BTreeMap;

fn analyze_with(config: Config, source: &str) -> radix_module::driver::AnalyzedModule {
    let session = Session::new(config);
    analyze_source(
        &session,
        "test.fab",
        source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports: BTreeMap::new(),
                file_interfaces: BTreeMap::new(),
                ..ModuleImportContract::default()
            },
        },
    )
    .expect("analysis must succeed")
}

fn analyze(source: &str) -> radix_module::driver::AnalyzedModule {
    analyze_with(
        Config::default().with_target(radix_module::codegen::Target::HirRust),
        source,
    )
}

#[test]
fn imported_json_struct_keeps_conversion_opt_in() {
    for annotated in [true, false] {
        let annotation = if annotated { "@ json {}" } else { "" };
        let producer = analyze(&format!(
            "@ publica\n{annotation}\ngenus Data {{ numerus value }}"
        ));
        let interface = extract_file_interface_with_identity(
            &producer,
            &["Data".to_owned()],
            "model.fab",
            None,
        )
        .expect("extract model interface");
        let consumer = analyze_source(
            &Session::new(Config::default().with_target(radix_module::codegen::Target::HirRust)),
            "consumer.fab",
            r#"importa ex "model:types" model
functio encode(model.Data value) → json { redde value ↦ json }
functio decode(json value) → model.Data { redde value ↦ model.Data }
"#,
            AnalyzeRequest {
                cli_program: None,
                contract: ModuleImportContract {
                    file_interfaces: BTreeMap::from([("model".to_owned(), interface)]),
                    ..ModuleImportContract::default()
                },
            },
        );
        assert_eq!(
            consumer.is_ok(),
            annotated,
            "annotation={annotated}: {:?}",
            consumer.err()
        );
    }
}

#[test]
fn extract_file_interface_snapshots_public_exports() {
    let analysis = analyze(
        r"@ publica
functio salve(textus nomen) → textus {
    redde nomen
}

@ privata
functio hidden() → vacuum {
    tacet
}

@ publica
genus Persona {
    textus nomen
}

@ publica
implendum Loquax {
    functio loqui() → textus
}
",
    );
    let export_names = [
        "salve".to_owned(),
        "Persona".to_owned(),
        "Loquax".to_owned(),
    ];
    let interface =
        extract_file_interface_with_identity(&analysis, &export_names, "test.fab", None)
            .expect("extract interface");

    assert!(matches!(
        interface.exports.get("salve").map(|export| &export.kind),
        Some(FileExportKind::Function(_))
    ));
    assert!(matches!(
        interface.exports.get("Persona").map(|export| &export.kind),
        Some(FileExportKind::Struct(_))
    ));
    assert!(matches!(
        interface.exports.get("Loquax").map(|export| &export.kind),
        Some(FileExportKind::Interface(_))
    ));
    // Private exports never appear even when named.
    assert!(!interface.exports.contains_key("hidden"));
}

#[test]
fn extract_file_interface_round_trips_public_commune_fields() {
    let analysis = analyze(
        r"@ publica
        discretio ShapeError {
            @ commune
            textus message

            Incompatible,
            ElementMismatch { numerus expected numerus actual }
        }
        ",
    );
    let interface = extract_file_interface_with_identity(
        &analysis,
        &["ShapeError".to_owned()],
        "test.fab",
        None,
    )
    .expect("extract interface");

    let Some(FileExportKind::Enum(export)) = interface
        .exports
        .get("ShapeError")
        .map(|export| &export.kind)
    else {
        panic!("public enum export missing: {interface:?}");
    };
    assert_eq!(
        export.shared_fields,
        vec![InterfaceStructField {
            name: "message".to_owned(),
            ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
            optional: false,
            required: true,
            default: None,
        }]
    );
    assert_eq!(export.variants.len(), 2);
    assert_eq!(export.variants[1].fields.len(), 2);

    analyze_source(
        &Session::new(Config::default().with_target(radix_module::codegen::Target::HirRust)),
        "consumer.fab",
        r#"importa ex "shape:errors" shape

functio render(shape.ShapeError e) → textus {
    redde e.message
}
"#,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports: BTreeMap::new(),
                file_interfaces: BTreeMap::from([("shape".to_owned(), interface.clone())]),
                ..ModuleImportContract::default()
            },
        },
    )
    .expect("commune field interface round trip");
}

#[test]
fn extract_file_interface_with_identity_sets_identity() {
    let analysis = analyze("functio salve() → vacuum { tacet }\n");
    let export_names = ["salve".to_owned()];
    let context = ExportIdentityContext {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["mod".to_owned()],
    };
    let interface =
        extract_file_interface_with_identity(&analysis, &export_names, "test.fab", Some(&context))
            .expect("extract interface");

    let identity = interface.identity.expect("identity set");
    assert_eq!(identity.provider, "package");
    assert_eq!(identity.package.as_deref(), Some("demo"));
    assert_eq!(identity.module_path, vec!["mod".to_owned()]);
}

#[test]
fn extract_callable_contracts_covers_only_public_functions() {
    let analysis = analyze(
        r"@ publica
functio open() → vacuum {
    tacet
}

@ privata
functio closed() → vacuum {
    tacet
}
",
    );
    let export_names = ["open".to_owned()];
    let contracts = extract_callable_contracts(&analysis, &export_names, "test.fab")
        .expect("extract contracts");

    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].name, "open");
    assert!(contracts[0].has_body);
}

#[test]
fn extract_callable_contracts_marks_bodyless_functions() {
    // Library interface analysis permits bodyless functions (bound by target
    // manifests, G4); the extracted contract carries that fact.
    let analysis = analyze_with(
        Config::default()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
        "functio salve() → textus\n",
    );
    let export_names = ["salve".to_owned()];
    let contracts = extract_callable_contracts(&analysis, &export_names, "test.fab")
        .expect("extract contracts");

    assert_eq!(contracts.len(), 1);
    assert!(!contracts[0].has_body);
}

#[test]
fn extract_callable_contracts_records_nucleum_fragment_role() {
    let analysis = analyze(
        r"
@ publica
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}
",
    );
    let contracts = extract_callable_contracts(&analysis, &["helper".to_owned()], "test.fab")
        .expect("extract fragment contract");
    assert_eq!(contracts.len(), 1);
    assert_eq!(
        contracts[0].nucleum_role,
        Some(radix_module::hir::NucleumRole::Fragment)
    );
    assert!(contracts[0].has_body);
}

#[test]
fn signature_only_fragment_export_fails_closed() {
    let session = radix_module::driver::Session::new(
        Config::default()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let Err(err) = radix_module::driver::analyze_source(
        &session,
        "test.fab",
        "@ publica\n@ nucleum fragment\nfunctio helper(fractus<f32> x) → fractus<f32>\n",
        AnalyzeRequest::default(),
    ) else {
        panic!("signature-only fragment must fail at compile time");
    };
    assert!(
        err.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "nucleum_fragment_effect")),
        "{err:?}"
    );
}

/// N4b: the S0-G6 seam never runs the solver — it materializes what the
/// interface snapshot receives. A param that is still a hole at export time
/// (no type-pattern match solved it) remains unrepresentable and fails closed
/// with the named `interface_export_unrepresentable` issue; the solved-union
/// shape (`Type::Union`) materializes instead (see `file_interface_s0r1_test`
/// — the producer→consumer chain proves the round trip).
#[test]
fn unsolved_hole_param_export_fails_closed() {
    let analysis = analyze("functio probe(_ u) → vacuum { tacet }\n");
    let Err(diagnostic) =
        extract_file_interface_with_identity(&analysis, &["probe".to_owned()], "test.fab", None)
    else {
        panic!("an unsolved hole param must fail closed at the seam");
    };
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| { arg.name == "issue" && arg.value == "interface_export_unrepresentable" }),
        "expected interface_export_unrepresentable, got {diagnostic:?}"
    );
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "reason" && arg.value.contains("InferenceTypeUnsupported")),
        "expected InferenceTypeUnsupported reason, got {diagnostic:?}"
    );
}

/// PKG-REP: an exported typed kernel twin — `@ nucleum`
/// `@ publica` with ordered `magnitudo` parameters and symbolic tensor-shape
/// witnesses — materializes as an `InterfaceCallable` through the package
/// export extraction path. The same-unit concrete caller keeps the DFV2-2
/// orphan admission silent so the extraction result is proven on both
/// partial and integrated trees; KPC-WIRE lands the library-cache device
/// parity this proof waited on, so the twin now carries its v2 contract.
#[test]
fn typed_kernel_twin_export_materializes_as_interface_callable() {
    let analysis = analyze(
        "@ publica\n@ nucleum\nfunctio gemma<magnitudo M, magnitudo K>(\n    tensor<f32, [M, K]> x\n) → tensor<f32, [M, K]> {\n    redde x\n}\n\nfunctio appellans(tensor<f32, [4, 8]> x) → tensor<f32, [4, 8]> {\n    redde gemma(x)\n}\n",
    );
    let interface =
        extract_file_interface_with_identity(&analysis, &["gemma".to_owned()], "test.fab", None)
            .expect("typed kernel twin export must be representable");

    let Some(FileExportKind::Function(callable)) =
        interface.exports.get("gemma").map(|export| &export.kind)
    else {
        panic!(
            "expected gemma function export, got {:?}",
            interface.exports
        );
    };
    let param_names: Vec<&str> = callable
        .type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(param_names, vec!["M", "K"]);
    let expected = InterfaceTypeSnapshot::Tensor(
        Box::new(InterfaceTypeSnapshot::SizedNumeric(
            Primitive::Fractus,
            radix_module::semantic::NumericWidth::F32,
        )),
        radix_module::file_interface::InterfaceIndexSnapshot::Tuple(vec![
            radix_module::file_interface::InterfaceIndexSnapshot::Param("M".to_owned()),
            radix_module::file_interface::InterfaceIndexSnapshot::Param("K".to_owned()),
        ]),
    );
    assert_eq!(callable.params[0].ty, expected);
    assert_eq!(*callable.ret, expected);
    // KPC-WIRE library-cache parity: the twin keeps its v2 device contract
    // through the package export extraction path — public Kernel with a
    // source body channel.
    let device = callable
        .device
        .as_ref()
        .expect("typed kernel twin must retain its InterfaceDeviceContract");
    assert_eq!(device.role, InterfaceDeviceRole::Kernel);
    assert_eq!(device.visibility, InterfaceDeviceVisibility::Public);
    assert_eq!(device.body, InterfaceBodyChannel::Source);
}
