use super::*;
use radix_module::driver::{analyze_source_with_cli_program_and_import_contract, Config, Session};
use radix_module::file_interface::FileExportKind;
use std::collections::BTreeMap;

fn analyze_with(config: Config, source: &str) -> radix_module::driver::AnalyzedModule {
    let session = Session::new(config);
    analyze_source_with_cli_program_and_import_contract(
        &session,
        "test.fab",
        source,
        None,
        BTreeMap::new(),
        BTreeMap::new(),
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
fn extract_file_interface_snapshots_public_exports() {
    let analysis = analyze(
        r#"@ publica
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
"#,
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
        r#"@ publica
functio open() → vacuum {
    tacet
}

@ privata
functio closed() → vacuum {
    tacet
}
"#,
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
    ) else {
        panic!("signature-only fragment must fail at compile time");
    };
    assert!(
        err.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "NUCLEUM_FRAGMENT_EFFECT")),
        "{err:?}"
    );
}
