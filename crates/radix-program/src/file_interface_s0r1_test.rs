//! S0-R1: interface declaration kind/order and applied size serialization
//! across the producer→consumer chain (semantic snapshot → program extraction
//! → module re-export → consumer import through the facade).

use super::*;
use radix_module::driver::{
    analyze_source, AnalyzeRequest, Config, ModuleImportContract, Session,
};
use radix_module::file_interface::{
    FileExportKind, InterfaceAliasExport, InterfaceGenericParam, InterfaceIndexSnapshot,
    InterfaceTypeSnapshot,
};
use radix_module::syntax::GenericParamKind;
use std::collections::BTreeMap;

const PROVIDER: &str = r#"@ publica
typus Vector<magnitudo N> = vector<fractus<f32>, N>

@ publica
typus Wrapper<T> = lista<T>

@ publica
functio origin() → Vector<3>
"#;

fn provider_interface() -> FileInterface {
    let session = Session::new(
        Config::default()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let analysis = analyze_source(
        &session,
        "provider.fab",
        PROVIDER,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract::default(),
        },
    )
    .expect("provider analysis must succeed");
    extract_file_interface_with_identity(
        &analysis,
        &[
            "Vector".to_owned(),
            "Wrapper".to_owned(),
            "origin".to_owned(),
        ],
        "provider.fab",
        None,
    )
    .expect("extract provider interface")
}

fn analyze_consumer(
    interface: &FileInterface,
    source: &str,
) -> Result<radix_module::driver::AnalyzedModule, Vec<radix_module::diagnostics::Diagnostic>> {
    let session = Session::new(
        Config::default()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    analyze_source(
        &session,
        "consumer.fab",
        source,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract {
                namespace_exports: BTreeMap::new(),
                file_interfaces: BTreeMap::from([("math".to_owned(), interface.clone())]),
                ..ModuleImportContract::default()
            },
        },
    )
}

#[test]
fn file_interface_preserves_generic_size_producer_declarations() {
    let interface = provider_interface();

    let FileExportKind::TypeAlias(alias) = &interface
        .exports
        .get("Vector")
        .map(|export| &export.kind)
        .expect("Vector export")
    else {
        panic!("expected alias export");
    };
    assert_eq!(
        alias,
        &InterfaceAliasExport {
            name: "Vector".to_owned(),
            params: vec![InterfaceGenericParam {
                name: "N".to_owned(),
                kind: GenericParamKind::Magnitudo,
            }],
            body: InterfaceTypeSnapshot::Vector(
                Box::new(InterfaceTypeSnapshot::SizedNumeric(
                    radix_module::semantic::Primitive::Fractus,
                    radix_module::semantic::NumericWidth::F32,
                )),
                InterfaceIndexSnapshot::Param("N".to_owned()),
            ),
        }
    );

    let FileExportKind::TypeAlias(wrapper) = &interface
        .exports
        .get("Wrapper")
        .map(|export| &export.kind)
        .expect("Wrapper export")
    else {
        panic!("expected alias export");
    };
    assert_eq!(
        wrapper.params,
        vec![InterfaceGenericParam {
            name: "T".to_owned(),
            kind: GenericParamKind::Typus,
        }]
    );
}

#[test]
fn file_interface_preserves_generic_size_producer_applied_signature() {
    let interface = provider_interface();
    let FileExportKind::Function(callable) = &interface
        .exports
        .get("origin")
        .map(|export| &export.kind)
        .expect("origin export")
    else {
        panic!("expected function export");
    };
    // The applied size argument serializes as a kinded Size, never a type.
    assert_eq!(
        callable.ret.as_ref(),
        &InterfaceTypeSnapshot::Applied(
            Box::new(InterfaceTypeSnapshot::Vector(
                Box::new(InterfaceTypeSnapshot::SizedNumeric(
                    radix_module::semantic::Primitive::Fractus,
                    radix_module::semantic::NumericWidth::F32,
                )),
                InterfaceIndexSnapshot::Param("N".to_owned()),
            )),
            vec![radix_module::file_interface::InterfaceAppliedArg::Size(
                InterfaceIndexSnapshot::Literal(3)
            )],
        )
    );
}

#[test]
fn file_interface_preserves_generic_size_consumer_round_trip() {
    let interface = provider_interface();
    // Positive: a literal size application through the imported alias lowers
    // through the full producer→consumer chain without diagnostics.
    analyze_consumer(
        &interface,
        r#"importa ex "math:vectors" math

functio consume() → math.Vector<3>
"#,
    )
    .expect("imported size application must analyze");
}

#[test]
fn file_interface_preserves_generic_size_kind_mismatch_fails_closed() {
    let interface = provider_interface();

    // Type argument in a `magnitudo` slot must fail closed in Faber analysis.
    let Err(diagnostics) = analyze_consumer(
        &interface,
        r#"importa ex "math:vectors" math

functio bad() → math.Vector<textus>
"#,
    ) else {
        panic!("type-for-size mismatch must fail closed");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "generic_argument_kind_mismatch")),
        "expected generic_argument_kind_mismatch, got {diagnostics:?}"
    );

    // Size argument in a `typus` slot must fail closed symmetrically.
    let Err(diagnostics) = analyze_consumer(
        &interface,
        r#"importa ex "math:vectors" math

functio bad() → math.Wrapper<3>
"#,
    ) else {
        panic!("size-for-type mismatch must fail closed");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "generic_argument_kind_mismatch")),
        "expected generic_argument_kind_mismatch, got {diagnostics:?}"
    );
}
