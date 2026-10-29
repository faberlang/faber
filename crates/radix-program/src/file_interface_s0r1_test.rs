//! S0-R1: interface declaration kind/order and applied size serialization
//! across the producer→consumer chain (semantic snapshot → program extraction
//! → module re-export → consumer import through the facade).
//!
//! N4b: the S0-G6 seam extension — an exported signature whose param solves to
//! a union (goal `flow-narrowing-and-match-patterns`, N4a semantics) arrives
//! at the file interface as `Type::Union` and materializes as
//! [`InterfaceTypeSnapshot::Union`] rather than failing unrepresentable. The
//! seam never runs the solver: it materializes what the snapshot receives
//! (the `lista<numerus> ∪ textus` shape is the `ct-hole` solve's member set).

use super::*;
use radix_module::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};
use radix_module::file_interface::{
    FileExportKind, InterfaceAliasExport, InterfaceGenericParam, InterfaceIndexSnapshot,
    InterfaceParam, InterfaceTypeSnapshot,
};
use radix_module::semantic::SemanticParamMode;
use radix_module::syntax::GenericParamKind;
use std::collections::BTreeMap;

const PROVIDER: &str = r"@ publica
typus Vector<magnitudo N> = vector<f32, magnitudo N>

@ publica
typus Wrapper<T> = lista<T>

@ publica
functio origin() → Vector<3>
";

fn provider_interface() -> FileInterface {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
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
            .with_latin_locale()
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

// ── N4b: solved-union materialization at the S0-G6 seam ─────────────────────

/// The `ct-hole` solve's member set: `fn probe(_ u)` narrowed by a type-pattern
/// match over `lista<numerus>` and `textus` arms solves the param to
/// `lista<numerus> ∪ textus`. The export side does not wait for the solver —
/// it materializes the union the interface snapshot receives.
const SOLVED_UNION_PROVIDER: &str = r"@ publica
functio probe(lista<numerus> ∪ textus u) → vacuum {
    tacet
}
";

fn solved_union_provider_interface() -> FileInterface {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let analysis = analyze_source(
        &session,
        "provider.fab",
        SOLVED_UNION_PROVIDER,
        AnalyzeRequest {
            cli_program: None,
            contract: ModuleImportContract::default(),
        },
    )
    .expect("provider analysis must succeed");
    extract_file_interface_with_identity(&analysis, &["probe".to_owned()], "provider.fab", None)
        .expect("extract provider interface")
}

#[test]
fn exported_signature_materializes_solved_union_param() {
    let interface = solved_union_provider_interface();
    let FileExportKind::Function(callable) = &interface
        .exports
        .get("probe")
        .map(|export| &export.kind)
        .expect("probe export")
    else {
        panic!("expected function export");
    };
    assert_eq!(
        callable.params,
        vec![InterfaceParam {
            ty: InterfaceTypeSnapshot::Union(vec![
                InterfaceTypeSnapshot::Primitive(radix_module::semantic::Primitive::Textus),
                InterfaceTypeSnapshot::Array(Box::new(InterfaceTypeSnapshot::Primitive(
                    radix_module::semantic::Primitive::Numerus
                ))),
            ]),
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
            axis: false,
        }],
        "the solved union must materialize as InterfaceTypeSnapshot::Union, never fail unrepresentable"
    );
}

#[test]
fn solved_union_signature_round_trips_to_the_consumer() {
    let interface = solved_union_provider_interface();
    // A consumer calls the imported callable with each union member. The
    // materialized union reconstructs as `Type::Union` on the consumer side,
    // so member calls unify against the parameter (SEM010 stays silent).
    analyze_consumer(
        &interface,
        r#"importa ex "math:union" math

functio call_list() → vacuum {
    sit lista<numerus> xs ← [1, 2]
    math.probe(xs)
}

functio call_text() → vacuum {
    math.probe("salve")
}
"#,
    )
    .expect("union-member calls must analyze through the round trip");
}

/// N4b end-to-end: the export-probe fixture is the ruling-6 acceptance story —
/// `fn probe(_ u)` with a type-pattern match. N4a's finalize writeback solves
/// the hole to `lista<numerus> ∪ textus` on the HIR param slot, so the seam
/// materializes the solved union instead of failing unrepresentable. The
/// fixture is English-surface (`fn`/`match`/`case`), so the en reader pack is
/// loaded explicitly — the radix unit harness compiles bare Latin by default.
#[test]
fn export_probe_fixture_materializes_solved_union_param() {
    let pack_path = radix_module::locale::workspace_locale_pack_path("en");
    let en_pack = radix_module::locale::LocalePack::from_toml_path(&pack_path)
        .expect("installed en pack should load");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_locale_pack(en_pack),
    );
    let analysis = analyze_source(
        &session,
        "export-probe.fab",
        include_str!("../test-fixtures/export-probe.fab"),
        AnalyzeRequest::default(),
    )
    .expect("fixture analysis must succeed");
    let interface = extract_file_interface_with_identity(
        &analysis,
        &["probe".to_owned()],
        "export-probe.fab",
        None,
    )
    .expect("extract fixture interface");

    let FileExportKind::Function(callable) = &interface
        .exports
        .get("probe")
        .map(|export| &export.kind)
        .expect("probe export")
    else {
        panic!("expected function export");
    };
    assert_eq!(
        callable.params,
        vec![InterfaceParam {
            ty: InterfaceTypeSnapshot::Union(vec![
                InterfaceTypeSnapshot::Primitive(radix_module::semantic::Primitive::Textus),
                InterfaceTypeSnapshot::Array(Box::new(InterfaceTypeSnapshot::Primitive(
                    radix_module::semantic::Primitive::Numerus
                ))),
            ]),
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
            axis: false,
        }],
        "the match-solved param hole must materialize as InterfaceTypeSnapshot::Union"
    );
}

// ── PKG-REP: typed kernel-twin round trip ───────────────────

/// The gradus kernel-twin declaration shape: an exported size-generic whose
/// tensor shape witnesses are the declared `magnitudo` parameters. The
/// same-unit concrete caller keeps orphan admission silent so the round trip
/// proves the interface, not the admission family (PKG-ADM owns that).
const TWIN_PROVIDER: &str = r"@ publica
functio gemma<magnitudo M, magnitudo K>(
    tensor<f32, [M, K]> x
) → tensor<f32, [M, K]> {
    redde x
}

functio appellans(tensor<f32, [4, 8]> x) → tensor<f32, [4, 8]> {
    redde gemma(x)
}
";

fn twin_provider_interface() -> FileInterface {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let analysis = analyze_source(
        &session,
        "provider.fab",
        TWIN_PROVIDER,
        AnalyzeRequest::default(),
    )
    .expect("provider analysis must succeed");
    let identity = crate::file_interface::ExportIdentityContext {
        provider: "gradus".to_owned(),
        package: Some("gradus".to_owned()),
        module_path: vec!["nn".to_owned()],
    };
    extract_file_interface_with_identity(
        &analysis,
        &["gemma".to_owned()],
        "provider.fab",
        Some(&identity),
    )
    .expect("twin provider interface must extract")
}

/// Producer identity and export name survive the interface, and the twin's
/// ordered size parameters arrive as `InterfaceCallable::type_params` with
/// symbolic `Param` shape witnesses (PKG-REP `done_when` (b)).
#[test]
fn twin_export_preserves_producer_identity_and_export_name() {
    let interface = twin_provider_interface();
    assert_eq!(
        interface
            .identity
            .as_ref()
            .expect("producer identity must ride the interface")
            .display_key(),
        "gradus::gradus::nn"
    );
    let Some(FileExportKind::Function(callable)) =
        interface.exports.get("gemma").map(|export| &export.kind)
    else {
        panic!("expected gemma export, got {:?}", interface.exports);
    };
    let param_names: Vec<&str> = callable
        .type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    assert_eq!(param_names, vec!["M", "K"]);
    assert!(matches!(
        callable.params[0].ty,
        InterfaceTypeSnapshot::Tensor(
            _,
            radix_module::file_interface::InterfaceIndexSnapshot::Tuple(ref dims)
        ) if dims.len() == 2
            && matches!(
                dims[0],
                radix_module::file_interface::InterfaceIndexSnapshot::Param(_)
            )
    ));
}

/// The consumer round trip through the full import contract (export surface
/// + interface, as the production per-unit `AnalyzeRequest` carries them): a
///   concrete-shaped call through the imported twin analyzes without
///   diagnostics — the portable signature carries the symbolic witnesses, and
///   the call binds `[4, 8]` against `[M, K]`.
#[test]
fn twin_signature_consumer_round_trip_binds_concrete_shapes() {
    let interface = twin_provider_interface();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(radix_module::codegen::Target::HirRust)
            .with_bodyless_functions(),
    );
    let contract = ModuleImportContract {
        namespace_exports: BTreeMap::from([(
            "nn".to_owned(),
            BTreeMap::from([(
                "gemma".to_owned(),
                radix_module::semantic::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        file_interfaces: BTreeMap::from([("nn".to_owned(), interface)]),
        namespace_seams: BTreeMap::from([("nn".to_owned(), false)]),
        namespace_declarations: BTreeMap::new(),
        default_import_names: BTreeMap::new(),
    };
    analyze_source(
        &session,
        "consumer.fab",
        r#"importa ex "gradus:nn" nn

functio consume(tensor<f32, [4, 8]> x) → tensor<f32, [4, 8]> {
    redde nn.gemma(x)
}
"#,
        AnalyzeRequest {
            cli_program: None,
            contract,
        },
    )
    .expect("imported twin call must analyze through the round trip");
}
