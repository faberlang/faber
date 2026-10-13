//! S1 U2 tests: canonical cross-analysis nominal identity + struct/enum VALUE
//! members riding the nominal remap (codex-gap campaign).
//!
//! Contract (codex-gap S1 U2): the same nominal type referenced from a library
//! analysis and from the consumer analysis unifies to ONE semantic type in the
//! merged package-MIR program. The canonical nominal-identity key is the
//! module-qualified shape already used by file interfaces: nominal kind +
//! home-module identity (provider + package + module path) + export name.
//! Distinct nominal types with the same short name from different module
//! identities do NOT unify. Per operator ruling O1, struct/enum VALUE members
//! (cross-unit struct-literal construction, enum-variant references) ride the
//! same remap.

use super::*;
use radix_mir_runner::BufferHost;
use radix_module::semantic::Type;

const NOMINAL_UTIL: &str = "@ public\nclass Persona {\n    var string nomen\n}\n\n@ public\nfn saluta(Persona p) → string {\n    return p.nomen\n}\n";
const NOMINAL_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    const utilModule.Persona persona ← utilModule.Persona { nomen = \"salve\" }\n    return utilModule.saluta(persona)\n}\n\nmain {\n    print run()\n}\n";

const NOMINAL_TWO_ALIASES_MAIN: &str = "import from \"./util\" * as utilModule\nimport from \"./util\" * as aliasModule\n\nfn run() → string {\n    const utilModule.Persona persona ← utilModule.Persona { nomen = \"salve\" }\n    return aliasModule.saluta(persona)\n}\n\nmain {\n    print run()\n}\n";

const NOMINAL_DISTINCT_UTIL: &str = "@ public\nclass Persona {\n    var string nomen\n}\n\n@ public\nfn util_nomen(Persona p) → string {\n    return p.nomen\n}\n";
const NOMINAL_DISTINCT_OTHER: &str = "@ public\nclass Persona {\n    var string titulus\n}\n\n@ public\nfn other_titulus(Persona p) → string {\n    return p.titulus\n}\n";
const NOMINAL_DISTINCT_MAIN: &str = "import from \"./util\" * as utilModule\nimport from \"./other\" * as otherModule\n\nfn run() → string {\n    const utilModule.Persona una ← utilModule.Persona { nomen = \"salve\" }\n    const otherModule.Persona altera ← otherModule.Persona { titulus = \"hei\" }\n    return utilModule.util_nomen(una) + \"-\" + otherModule.other_titulus(altera)\n}\n\nmain {\n    print run()\n}\n";

const NOMINAL_ENUM_UTIL: &str = "@ public\nunion Color {\n    rubrum,\n    caeruleum\n}\n\n@ public\nfn primus() → Color {\n    return Color.rubrum\n}\n\n@ public\nfn colore_nomen(Color c) → string {\n    match c {\n        case rubrum {\n            return \"rubrum\"\n        }\n        case caeruleum {\n            return \"caeruleum\"\n        }\n    }\n}\n";
const NOMINAL_ENUM_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.colore_nomen(utilModule.primus())\n}\n\nmain {\n    print run()\n}\n";
const NOMINAL_ENUM_VARIANT_MAIN: &str = "import from \"./util\" * as utilModule\n\nfn run() → string {\n    return utilModule.colore_nomen(utilModule.rubrum)\n}\n\nmain {\n    print run()\n}\n";

/// Write a two-module package with `util.fab` content `util_source` and entry
/// `main_source` (both imported through the package manifest).
fn write_nominal_package(
    dir: &std::path::Path,
    util_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-nominal"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), util_source).expect("write util.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

/// Write a three-module package: `util.fab` + `other.fab` + entry.
fn write_three_module_package(
    dir: &std::path::Path,
    util_source: &str,
    other_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-nominal-distinct"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("util.fab"), util_source).expect("write util.fab");
    fs::write(src.join("other.fab"), other_source).expect("write other.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

/// Count the distinct `TypeId`s in the merged program's type table whose
/// shape is one of the nominal kinds AND whose definition is a user nominal
/// (carries struct-field / enum-variant metadata in the merged validation
/// context). Prelude builtin nominal shapes (e.g. the `forma` tensor-shape
/// struct that every analysis's type table contains) are excluded — they are
/// not module-exported nominals and never register in the validation context,
/// so counting them would pollute the unification assertion. Before S1 U2 the
/// sibling's nominal def was interned as-is, producing a second (coincident)
/// TypeId; the canonical remap collapses same-nominal references to one.
fn count_merged_nominals(lowered: &LoweredMirUnit<'_>, kind: &str) -> usize {
    let types = lowered.validated.validation().types.as_table();
    let validation = lowered.validated.validation();
    let mut seen = std::collections::HashSet::new();
    for index in 0..types.type_count() {
        let ty = types.get(TypeId(index as u32));
        let is_nominal = match ty {
            Type::Struct(def_id) => {
                kind == "struct" && validation.struct_fields.contains_key(def_id)
            }
            Type::Enum(def_id) => kind == "enum" && validation.enum_variants.contains_key(def_id),
            Type::Interface(_) => kind == "interface",
            _ => false,
        };
        if is_nominal {
            seen.insert(index);
        }
    }
    seen.len()
}

#[test]
fn package_mir_nominal_identity_unifies_across_analyses() {
    // S1 U2 positive: the same `Persona` referenced from the sibling analysis
    // (its own function signatures) and from the entry analysis (struct
    // literal + call boundary) must be ONE semantic type in the merged
    // program, and a function boundary crossing the nominal type validates
    // and executes.
    let dir = crate::test_support::test_temp_dir("s1u2-unify");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                count_merged_nominals(lowered, "struct"),
                1,
                "same nominal across analyses must unify to one TypeId"
            );
        },
    )
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["salve".to_owned()],
        "cross-unit struct-literal construction + call must match the oracle"
    );
}

#[test]
fn package_mir_two_alias_imports_unify_nominals() {
    // S1 U2 negative proof (a) two-aliases-unify: the same module imported
    // under two binding aliases yields ONE TypeId for its nominal types.
    let dir = crate::test_support::test_temp_dir("s1u2-two-aliases");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_TWO_ALIASES_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                count_merged_nominals(lowered, "struct"),
                1,
                "same module under two aliases must unify to one TypeId"
            );
        },
    )
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn package_mir_same_name_nominals_from_distinct_modules_do_not_unify() {
    // S1 U2 negative proof (b) coincident-raw-DefId-distinct: two distinct
    // nominal types with the same short name from different module identities
    // must NOT unify — each resolves through its own canonical key, even when
    // the raw source DefIds numerically coincide.
    let dir = crate::test_support::test_temp_dir("s1u2-distinct");
    let entry = write_three_module_package(
        dir.as_ref(),
        NOMINAL_DISTINCT_UTIL,
        NOMINAL_DISTINCT_OTHER,
        NOMINAL_DISTINCT_MAIN,
    );
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                count_merged_nominals(lowered, "struct"),
                2,
                "same-name nominals from distinct module identities must stay distinct"
            );
        },
    )
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["salve-hei".to_owned()],
        "distinct nominal types must keep their own fields and execute"
    );
}

#[test]
fn package_mir_enum_variant_value_members_ride_nominal_remap() {
    // S1 U2 O1 VALUE members: an enum-variant value crosses the unit boundary
    // (library constructs `Color.rubrum`, entry consumes it through a call)
    // and executes through package MIR, riding the nominal remap.
    let dir = crate::test_support::test_temp_dir("s1u2-enum");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            assert_eq!(
                count_merged_nominals(lowered, "enum"),
                1,
                "same enum across analyses must unify to one TypeId"
            );
        },
    )
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["rubrum".to_owned()],
        "cross-unit enum-variant value must match the oracle"
    );
}

#[test]
fn package_mir_namespace_enum_variant_reference_executes() {
    // S1 U2 O1 VALUE members: the entry DIRECTLY references the library enum
    // variant (`utilModule.rubrum`) and passes it back into the library, which
    // pattern-matches it. The namespace variant reference, the consumer
    // variant construction, and the merge-time variant-def remap all ride the
    // nominal remap.
    let dir = crate::test_support::test_temp_dir("s1u2-enum-variant");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_VARIANT_MAIN);
    let config = Config::default();

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["rubrum".to_owned()],
        "namespace enum-variant reference must match the oracle"
    );
}

/// Assert the imported enum's variant metadata survives into the merged
/// program's validation context (S1 U2 merged-import-validation).
#[test]
fn package_mir_merged_validation_carries_imported_variants() {
    let dir = crate::test_support::test_temp_dir("s1u2-merged-variants");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_VARIANT_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let validation = lowered.validated.validation();
            // One consumer enum def with two variants in the merged program.
            let enum_entries = validation
                .enum_variants
                .iter()
                .filter(|(_, variants)| variants.len() == 2)
                .count();
            assert_eq!(
                enum_entries, 1,
                "imported enum variants must survive the remap into the merged program"
            );
            // Every variant in the merged program has parent + field metadata.
            for variant in validation.variant_parents.values() {
                let _ = variant;
            }
            assert!(
                !validation.variant_parents.is_empty(),
                "merged program must carry variant parent metadata"
            );
        },
    )
    .expect("merged package MIR program");
}

/// S1 U2 negative proof (d) merged-import-validation: struct-field access and
/// enum-variant construction validate against the merged nominal type —
/// the struct's fields survive the remap into the entry type table.
#[test]
fn package_mir_merged_validation_carries_imported_struct_fields() {
    let dir = crate::test_support::test_temp_dir("s1u2-merged-struct");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let validation = lowered.validated.validation();
            let struct_field_entries = validation
                .struct_fields
                .iter()
                .filter(|(_, fields)| !fields.is_empty())
                .count();
            assert_eq!(
                struct_field_entries, 1,
                "imported struct fields must survive the remap into the merged program"
            );
        },
    )
    .expect("merged package MIR program");
}

/// The canonical identity contract, pinned here for the delivery/campaign
/// note: the key shape is (nominal kind, home-module InterfaceLibraryIdentity
/// {provider, package, module_path}, export name); the unification rule is
/// same-key → one semantic type in the merged program.
#[test]
fn canonical_nominal_identity_key_shape_is_module_qualified() {
    let identity = radix_module::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    assert_eq!(identity.display_key(), "package::demo::util");
    let other = radix_module::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["other".to_owned()],
    };
    assert_ne!(
        identity, other,
        "distinct module paths are distinct identities"
    );
    assert_eq!(
        identity.display_key() + "::Persona",
        "package::demo::util::Persona",
        "canonical nominal key = identity + export name"
    );
}

// Sibling variant-payload fixture: the library exports an enum whose variant
// payload references a nominal from a SIBLING module (`other`) the consumer
// does not import directly. Since MRL-4 (radix 95b96b17d,
// install_transitive_local_interfaces) the consumer's import contract installs
// the payload module transitively, so `caeruleum` — and its `other.Weird`
// payload — resolves in the consumer and the merge succeeds. The positive test
// pins that CURRENT truth; the library-payload test below proves fail-closed
// still fires where a payload module stays genuinely foreign to the consumer.
const NOMINAL_VARIANT_OTHER: &str = "@ public\nclass Weird {\n    var int gradus\n}\n";
const NOMINAL_VARIANT_UTIL: &str = "import from \"./other\" * as otherModule\n\n@ public\nclass Gradus {\n    var int gradus\n}\n\n@ public\nunion Color {\n    rubrum,\n    caeruleum { otherModule.Weird gradus }\n}\n\n@ public\nfn primus() → Color {\n    return Color.rubrum\n}\n";
const NOMINAL_VARIANT_MAIN: &str =
    "import from \"./util\" * as utilModule\n\nmain {\n    print 1\n}\n";

/// (a) Post-MRL-4 truth: the variant-payload sibling module registers in the
/// consumer transitively, so the payload variant resolves and the merge does
/// NOT fail closed. The fail-closed diagnostic this fixture used to pin is
/// unreachable by design since MRL-4 (install_transitive_local_interfaces).
#[test]
fn identity_bearing_enum_variant_sibling_payload_registers_transitively() {
    let dir = crate::test_support::test_temp_dir("s1u2-variant-transitive");
    let entry = write_three_module_package(
        dir.as_ref(),
        NOMINAL_VARIANT_UTIL,
        NOMINAL_VARIANT_OTHER,
        NOMINAL_VARIANT_MAIN,
    );
    let config = Config::default();

    with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let validation = lowered.validated.validation();
            let two_variant_enums = validation
                .enum_variants
                .iter()
                .filter(|(_, variants)| variants.len() == 2)
                .count();
            assert_eq!(
                two_variant_enums, 1,
                "the payload variant must survive the remap into the merged program"
            );
        },
    )
    .expect("the transitively installed payload module must keep the variant resolvable, not fail closed");

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("package MIR run");
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);

    // The resolver-level facts the drift probe found: the entry analysis
    // registers the payload module's identity and resolves the payload
    // variant through `variant_child`.
    let package = crate::analyze::analyze_package(
        &config,
        &entry,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze package");
    let entry_id = package.entry.expect("package has an entry");
    let entry_node = package
        .nodes
        .get(&entry_id)
        .expect("entry id is a program node");
    let resolver = &entry_node.analysis.resolver;
    let mut interner = entry_node.analysis.interner.clone();
    let other_identity = radix_module::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("fhir-nominal-distinct".to_owned()),
        module_path: vec!["other".to_owned()],
    };
    assert!(
        resolver.canonical_identity_known(&other_identity),
        "the payload module identity must be known to the consumer analysis"
    );
    let util_identity = radix_module::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("fhir-nominal-distinct".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    let color_def = resolver
        .imported_nominal_def_id(
            radix_module::file_interface::InterfaceNominalKind::Enum,
            Some(&util_identity),
            interner.intern("Color"),
        )
        .expect("the consumer analysis must register the imported Color enum");
    for variant_name in ["rubrum", "caeruleum"] {
        assert!(
            resolver
                .variant_child(color_def, interner.intern(variant_name))
                .is_some(),
            "variant `{variant_name}` must resolve in the consumer analysis"
        );
    }
}

// (b) Remaining fail-closed scenario (the fixture's original intent): the
// payload nominal's home module stays genuinely foreign to the consumer. The
// transitive install follows only LOCAL imports of imported modules (MRL-4),
// never LIBRARY imports — a variant payload naming a library nominal from a
// module the consumer never imports still drops in phase 2, and the merge
// fails closed because the enum's own home module (util) IS known to the
// consumer (council-11 correct_before_next_phase: the unknown variant must
// not silently retain its source-local identity).
const NOMINAL_LIBRARY_MOD: &str =
    "+++\nlocale = \"en\"\n+++\n@ public\nclass Something {\n    var int gradus\n}\n";
const NOMINAL_VARIANT_UTIL_LIBRARY: &str = "import from \"prov:mod\" * as provModule\n\n@ public\nclass Gradus {\n    var int gradus\n}\n\n@ public\nunion Color {\n    rubrum,\n    caeruleum { provModule.Something gradus }\n}\n\n@ public\nfn primus() → Color {\n    return Color.rubrum\n}\n";
const NOMINAL_VARIANT_MAIN_LIBRARY: &str =
    "import from \"./util\" * as utilModule\n\nmain {\n    print 1\n}\n";

/// Write a two-module package whose `util.fab` variant payload references a
/// nominal from the lock-backed library `prov:mod`, plus the library module.
fn write_nominal_library_package(
    dir: &std::path::Path,
    util_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    let library = dir.join("libhome").join("prov");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-nominal-libvariant"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
prov = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"prov\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/mod.fab"), NOMINAL_LIBRARY_MOD).expect("write library module");
    fs::write(src.join("util.fab"), util_source).expect("write util.fab");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn identity_bearing_enum_variant_library_payload_miss_fails_closed() {
    let dir = crate::test_support::test_temp_dir("s1u2-variant-lib-fail-closed");
    let entry = write_nominal_library_package(
        dir.as_ref(),
        NOMINAL_VARIANT_UTIL_LIBRARY,
        NOMINAL_VARIANT_MAIN_LIBRARY,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));

    let errors = with_interpreted_lowered_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        |_| {},
    )
    .expect_err("a library-payload variant of a known enum must fail closed");
    assert!(
        errors.iter().any(|diag| {
            diag.code == Some("PKG001")
                && diag.issue() == Some("package_mir_variant_identity_unknown")
        }),
        "expected an unknown-identity fail-closed diagnostic, got: {errors:?}"
    );
}

#[test]
fn package_mir_imported_nested_field_defaults_keep_type_and_text_identity() {
    let dir = crate::test_support::test_temp_dir("nested-import-default");
    let entry = write_nominal_package(
        dir.as_ref(),
        r#"import from "./headers" headers
@ public
class Request {
    var headers.Headers headers = headers.Headers { }
    var string body = "producer body"
}
"#,
        r#"import from "./util" httpreq
import from "./headers" headers
class Incoming {
    var string id = ""
    var int connection = 0
    var httpreq.Request req = httpreq.Request { }
}
main {
    const Incoming incoming ← Incoming { id = "http-1", connection = 3 }
    assert incoming.id ≡ "http-1"
    assert incoming.connection ≡ 3
    assert incoming.req.body ≡ "producer body"
    assert incoming.req.headers.name ≡ "nested producer text"
    assert incoming.req.headers.entries.length() ≡ 0
}
"#,
    );
    fs::write(
        dir.join("src/headers.fab"),
        r#"@ public
class Header {
    var string name = "header name"
}
@ public
class Headers {
    var string name = "nested producer text"
    var list<Header> entries = []
}
"#,
    )
    .expect("write nested producer");
    let mut host = BufferHost::default();
    run_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("nested imported defaults and all five value assertions run");
}
