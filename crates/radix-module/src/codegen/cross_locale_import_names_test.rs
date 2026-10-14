//! Cross-locale imported names use the exporter's canonical spelling.
//!
//! A `la` consumer imports a type exported by an `en` unit. The consumer's
//! reader pack projects the export's canonical spelling onto its own surface
//! (`Socket` → `Nexus`), and the type checker resolves the surface spelling to
//! the exporting unit's `DefId`. The flattened Go/TS package declares the type
//! under the exporter's canonical name, so backend name registration must map
//! the surface member through `Resolver::imported_export_canonical_name`
//! instead of registering the consumer surface as the emitted identifier —
//! the Rust backend's `register_library_type_paths` seam does the same.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, ModuleImportContract, Session};
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity, InterfaceStructExport, InterfaceStructField,
    InterfaceTypeSnapshot,
};
use crate::locale::{latin_locale_pack, merge_library_locale_packs};
use crate::semantic::Primitive;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// In-tree library root owning the `la` member fragment (`Socket` → `Nexus`).
fn norma_library_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/norma")
}

/// The exporting `norma:websocket` interface: a struct exported as `Socket`.
fn socket_interface() -> FileInterface {
    let mut interface = FileInterface::new().with_identity(InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("norma".to_owned()),
        module_path: vec!["websocket".to_owned()],
    });
    interface.insert(FileExport {
        name: "Socket".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Socket".to_owned(),
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
    interface
}

/// One `la` consumer session whose reader pack carries the norma member rows.
fn la_consumer_session(target: Target) -> Session {
    let mut pack = latin_locale_pack();
    merge_library_locale_packs(&mut pack, [norma_library_root()]).expect("merge la member fragment");
    Session::new(
        Config::default()
            .with_target(target)
            .with_resolved_locale_pack(pack),
    )
}

/// The consumer names the imported struct by its `la` surface spelling.
const LA_CONSUMER_SOURCE: &str = "\
importa ex \"./websocket\" websocket\n\
\n\
incipit {\n\
    fixum websocket.Nexus s ← websocket.Nexus { gradus = 1 }\n\
    nota s.gradus\n\
}\n";

fn la_consumer_contract() -> ModuleImportContract {
    ModuleImportContract {
        namespace_exports: BTreeMap::from([(
            "websocket".to_owned(),
            BTreeMap::from([
                ("Socket".to_owned(), VisibilityTier::Publica),
                ("Nexus".to_owned(), VisibilityTier::Publica),
            ]),
        )]),
        file_interfaces: BTreeMap::from([("websocket".to_owned(), socket_interface())]),
        ..ModuleImportContract::default()
    }
}

fn compile_consumer(target: Target) -> crate::CompileResult {
    let session = la_consumer_session(target);
    let request = ModuleCompileRequest { contract: la_consumer_contract(), ..ModuleCompileRequest::default() };
    let result = crate::driver::compile(&session, "consumer.fab", LA_CONSUMER_SOURCE, request);
    assert!(result.success(), "la consumer should compile: {:?}", result.diagnostics);
    result
}

#[test]
fn cross_locale_import_names_go_uses_exporter_canonical_spelling() {
    let result = compile_consumer(Target::HirGo);
    let Some(crate::Output::Go(out)) = result.output else {
        panic!("expected Go output");
    };
    assert!(
        out.code.contains("Socket{"),
        "Go must name the exported type by its canonical spelling:\n{}",
        out.code
    );
    assert!(
        !out.code.contains("Nexus"),
        "Go must not name the exported type by the consumer surface spelling:\n{}",
        out.code
    );
}

#[test]
fn cross_locale_import_names_ts_uses_exporter_canonical_spelling() {
    let result = compile_consumer(Target::HirTypeScript);
    let Some(crate::Output::TypeScript(out)) = result.output else {
        panic!("expected TypeScript output");
    };
    assert!(
        out.code.contains("websocket.Socket"),
        "TS must name the exported type by its canonical spelling:\n{}",
        out.code
    );
    assert!(
        !out.code.contains("Nexus"),
        "TS must not name the exported type by the consumer surface spelling:\n{}",
        out.code
    );
}
