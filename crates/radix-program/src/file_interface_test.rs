use super::*;
use radix_module::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};
use radix_module::file_interface::{
    FileExportKind, InterfaceBodyChannel, InterfaceDeviceRole, InterfaceDeviceVisibility,
    InterfaceIndexSnapshot, InterfaceStructField, InterfaceTypeSnapshot,
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
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust),
        source,
    )
}

#[test]
fn imported_json_struct_keeps_conversion_opt_in() {
    for annotated in [true, false] {
        let annotation = if annotated { "@ json {}" } else { "" };
        let producer = analyze(&format!(
            "@ publica\n{annotation}\ngenus Data {{ varia numerus value }}"
        ));
        let interface = extract_file_interface_with_identity(
            &producer,
            &["Data".to_owned()],
            "model.fab",
            None,
        )
        .expect("extract model interface");
        let consumer = analyze_source(
            &Session::new(
                Config::default()
                    .with_latin_locale()
                    .with_target(radix_module::codegen::Target::HirRust),
            ),
            "consumer.fab",
            r#"importa ex "model:types" model
functio encode(model.Data value) → json ⇥ textus { redde value ↦ json }
functio decode(json value) → model.Data ⇥ textus { redde value ↦ model.Data }
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
    varia textus nomen
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
            modifier: None,
            visibility: radix_module::syntax::MemberVisibility::Publica,
        }]
    );
    assert_eq!(export.variants.len(), 2);
    assert_eq!(export.variants[1].fields.len(), 2);

    analyze_source(
        &Session::new(
            Config::default()
                .with_latin_locale()
                .with_target(radix_module::codegen::Target::HirRust),
        ),
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
            .with_latin_locale()
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
functio helper(f32 x) → f32 {
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
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let Err(err) = radix_module::driver::analyze_source(
        &session,
        "test.fab",
        "@ publica\n@ nucleum fragment\nfunctio helper(f32 x) → f32\n",
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

const NONDUM_PRODUCER: &str = r#"@ publica
@ nondum "pending host dispatch"
functio later(textus s) → textus

@ publica
@ nondum ts
functio later_ts(textus s) → textus {
    redde s
}

@ publica
functio nunc(textus s) → textus {
    redde s
}
"#;

fn nondum_producer_interface() -> radix_module::file_interface::FileInterface {
    let analysis = analyze(NONDUM_PRODUCER);
    let export_names = ["later".to_owned(), "later_ts".to_owned(), "nunc".to_owned()];
    extract_file_interface_with_identity(&analysis, &export_names, "model.fab", None)
        .expect("extract interface")
}

#[test]
fn extract_file_interface_carries_nondum_gates() {
    let interface = nondum_producer_interface();

    for name in ["later", "later_ts", "nunc"] {
        assert!(
            matches!(
                interface.exports.get(name).map(|export| &export.kind),
                Some(FileExportKind::Function(_))
            ),
            "{name} stays exported so importers can resolve and refuse it"
        );
    }
    assert_eq!(
        interface.unavailable.get("later"),
        Some(&vec![radix_module::file_interface::InterfaceUnavailable {
            target: None,
            reason: Some("pending host dispatch".to_owned()),
        }])
    );
    assert_eq!(
        interface.unavailable.get("later_ts"),
        Some(&vec![radix_module::file_interface::InterfaceUnavailable {
            target: Some("ts".to_owned()),
            reason: None,
        }])
    );
    assert!(!interface.unavailable.contains_key("nunc"));
}

fn analyze_nondum_consumer(source: &str) -> Result<(), Vec<radix_module::diagnostics::Diagnostic>> {
    analyze_source(
        &Session::new(
            Config::default()
                .with_latin_locale()
                .with_target(radix_module::codegen::Target::HirRust),
        ),
        "consumer.fab",
        source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                file_interfaces: BTreeMap::from([(
                    "model".to_owned(),
                    nondum_producer_interface(),
                )]),
                ..ModuleImportContract::default()
            },
        },
    )
    .map(|_| ())
}

fn refused_functions(diagnostics: &[radix_module::diagnostics::Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == Some("SEM017"))
        .filter_map(|diagnostic| {
            diagnostic
                .args
                .iter()
                .find(|arg| arg.name == "function")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

#[test]
fn imported_nondum_function_is_refused_through_the_extracted_interface() {
    let diagnostics = analyze_nondum_consumer(
        "importa ex \"model:types\" model\nincipit { nota model.later(\"x\") }\n",
    )
    .expect_err("a call to an imported nondum function must not compile");
    assert_eq!(
        refused_functions(&diagnostics),
        vec!["model.later".to_owned()]
    );
}

#[test]
fn imported_nondum_function_value_use_is_refused_through_the_extracted_interface() {
    let diagnostics = analyze_nondum_consumer(
        "importa ex \"model:types\" model\nincipit {\n    sit _ f ← model.later\n    tacet\n}\n",
    )
    .expect_err("a value use of an imported nondum function must not compile");
    assert_eq!(
        refused_functions(&diagnostics),
        vec!["model.later".to_owned()]
    );
}

#[test]
fn imported_nondum_function_is_usable_where_its_gate_does_not_apply() {
    // The `ts` gate on `later_ts` does not cover the Rust target.
    analyze_nondum_consumer(
        "importa ex \"model:types\" model\nincipit {\n    nota model.nunc(\"x\")\n    nota model.later_ts(\"y\")\n}\n",
    )
    .expect("ungated and other-target-gated functions compile");
}

const NONDUM_GENUS_PRODUCER: &str = r#"@ publica
genus Nexus {
    varia numerus id

    @ nondum "pending host dispatch"
    functio claude() → vacuum

    @ nondum ts
    functio mitte() → vacuum {
        tacet
    }

    functio lege() → numerus {
        redde ego.id
    }
}
"#;

fn nondum_genus_interface() -> radix_module::file_interface::FileInterface {
    let analysis = analyze(NONDUM_GENUS_PRODUCER);
    extract_file_interface_with_identity(&analysis, &["Nexus".to_owned()], "model.fab", None)
        .expect("extract interface")
}

#[test]
fn extract_file_interface_carries_nondum_method_gates() {
    let interface = nondum_genus_interface();
    let Some(FileExportKind::Struct(strukt)) =
        interface.exports.get("Nexus").map(|export| &export.kind)
    else {
        panic!("Nexus exports as a struct");
    };
    let gates = |name: &str| {
        strukt
            .methods
            .iter()
            .find(|method| method.name == name)
            .unwrap_or_else(|| panic!("method {name} stays exported"))
            .unavailable
            .clone()
    };
    assert_eq!(
        gates("claude"),
        vec![radix_module::file_interface::InterfaceUnavailable {
            target: None,
            reason: Some("pending host dispatch".to_owned()),
        }]
    );
    assert_eq!(
        gates("mitte"),
        vec![radix_module::file_interface::InterfaceUnavailable {
            target: Some("ts".to_owned()),
            reason: None,
        }]
    );
    assert_eq!(
        gates("lege"),
        [] as [radix_module::file_interface::InterfaceUnavailable; 0]
    );
}

#[test]
fn imported_nondum_method_is_refused_through_the_extracted_interface() {
    let analyze_consumer = |body: &str| {
        analyze_source(
            &Session::new(Config::default().with_latin_locale().with_target(radix_module::codegen::Target::HirRust)),
            "consumer.fab",
            &format!(
                "importa ex \"model:types\" model\nincipit {{\n    varia model.Nexus n ← model.Nexus {{ id = 1 }}\n{body}\n}}\n"
            ),
            AnalyzeRequest {
                cli_program: None,
                contract: ModuleImportContract {
                    file_interfaces: BTreeMap::from([(
                        "model".to_owned(),
                        nondum_genus_interface(),
                    )]),
                    ..ModuleImportContract::default()
                },
            },
        )
        .map(|_| ())
    };
    let diagnostics = analyze_consumer("    n.claude()")
        .expect_err("a call to an imported nondum method must not compile");
    assert_eq!(
        refused_functions(&diagnostics),
        vec!["Nexus.claude".to_owned()]
    );

    // The ts gate on `mitte` does not cover Rust, and `lege` is ungated.
    analyze_consumer("    n.mitte()\n    nota n.lege()")
        .expect("ungated and other-target-gated methods compile");
}

const SIZED_PRODUCER: &str = r"@ publica
genus Block {
    varia numerus scale
    varia lista<numerus, magnitudo 4> qs
}

@ publica
typus Quad = lista<numerus, magnitudo 4>

@ publica
typus Window = lista<numerus, magnitudo 2…5>
";

fn sized_producer_interface() -> radix_module::file_interface::FileInterface {
    let producer = analyze(SIZED_PRODUCER);
    extract_file_interface_with_identity(
        &producer,
        &["Block".to_owned(), "Quad".to_owned(), "Window".to_owned()],
        "sized.fab",
        None,
    )
    .expect("a public export carrying a sized or ranged list extracts")
}

fn analyze_sized_consumer(source: &str) -> Result<(), Vec<radix_module::diagnostics::Diagnostic>> {
    analyze_source(
        &Session::new(
            Config::default()
                .with_latin_locale()
                .with_target(radix_module::codegen::Target::HirRust),
        ),
        "consumer.fab",
        source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                file_interfaces: BTreeMap::from([("sized".to_owned(), sized_producer_interface())]),
                ..ModuleImportContract::default()
            },
        },
    )
    .map(|_| ())
}

#[test]
fn sized_and_ranged_list_exports_snapshot_with_their_size_facts() {
    let interface = sized_producer_interface();

    let Some(FileExportKind::Struct(block)) =
        interface.exports.get("Block").map(|export| &export.kind)
    else {
        panic!("Block export missing: {interface:?}");
    };
    let qs = block
        .fields
        .iter()
        .find(|field| field.name == "qs")
        .expect("qs field");
    assert!(matches!(
        &qs.ty,
        InterfaceTypeSnapshot::BoundedArray(_, InterfaceIndexSnapshot::Literal(4))
    ));

    let Some(FileExportKind::TypeAlias(quad)) =
        interface.exports.get("Quad").map(|export| &export.kind)
    else {
        panic!("Quad alias export missing");
    };
    assert!(matches!(
        &quad.body,
        InterfaceTypeSnapshot::BoundedArray(_, InterfaceIndexSnapshot::Literal(4))
    ));

    let Some(FileExportKind::TypeAlias(window)) =
        interface.exports.get("Window").map(|export| &export.kind)
    else {
        panic!("Window alias export missing");
    };
    assert!(matches!(
        &window.body,
        InterfaceTypeSnapshot::RangedArray {
            min: 2,
            max: Some(5),
            ..
        }
    ));
}

#[test]
fn imported_sized_list_field_and_aliases_keep_their_size_rules() {
    analyze_sized_consumer(
        r#"importa ex "sized:types" sized

functio lege(sized.Block b) → numerus {
    redde b.qs[2] + b.scale
}

functio quad(sized.Quad q) → lista<numerus, magnitudo 4> {
    redde q
}

functio fenestra(sized.Window w) → lista<numerus, magnitudo 2…5> {
    redde w
}
"#,
    )
    .expect("imported sized and ranged lists keep their identity");

    // The size rules still bind on the importing side: a four-slot field is
    // not an eight-slot list, and a range never holds fewer than its minimum.
    analyze_sized_consumer(
        r#"importa ex "sized:types" sized

functio lege(sized.Block b) → lista<numerus, magnitudo 8> {
    redde b.qs
}
"#,
    )
    .expect_err("a four-slot list field must not satisfy an eight-slot list");

    analyze_sized_consumer(
        r#"importa ex "sized:types" sized

functio vacua() → numerus {
    fixum sized.Window w ← [1]
    redde w[0]
}
"#,
    )
    .expect_err("a one-element list never fits a range with minimum two");
}
