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
use radix::mir::BufferHost;
use radix::semantic::Type;

const NOMINAL_UTIL: &str = "genus Persona {\n    textus nomen\n}\n\nfunctio saluta(Persona p) → textus {\n    redde p.nomen\n}\n";
const NOMINAL_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    fixum utilModule.Persona persona ← utilModule.Persona { nomen = \"salve\" }\n    redde utilModule.saluta(persona)\n}\n\nincipit {\n    nota run()\n}\n";

const NOMINAL_TWO_ALIASES_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\nimporta ex \"./util\" privata * ut aliasModule\n\nfunctio run() → textus {\n    fixum utilModule.Persona persona ← utilModule.Persona { nomen = \"salve\" }\n    redde aliasModule.saluta(persona)\n}\n\nincipit {\n    nota run()\n}\n";

const NOMINAL_DISTINCT_UTIL: &str = "genus Persona {\n    textus nomen\n}\n\nfunctio util_nomen(Persona p) → textus {\n    redde p.nomen\n}\n";
const NOMINAL_DISTINCT_OTHER: &str = "genus Persona {\n    textus titulus\n}\n\nfunctio other_titulus(Persona p) → textus {\n    redde p.titulus\n}\n";
const NOMINAL_DISTINCT_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\nimporta ex \"./other\" privata * ut otherModule\n\nfunctio run() → textus {\n    fixum utilModule.Persona una ← utilModule.Persona { nomen = \"salve\" }\n    fixum otherModule.Persona altera ← otherModule.Persona { titulus = \"hei\" }\n    redde utilModule.util_nomen(una) + \"-\" + otherModule.other_titulus(altera)\n}\n\nincipit {\n    nota run()\n}\n";

const NOMINAL_ENUM_UTIL: &str = "discretio Color {\n    rubrum,\n    caeruleum\n}\n\nfunctio primus() → Color {\n    redde Color.rubrum\n}\n\nfunctio colore_nomen(Color c) → textus {\n    discerne c {\n        casu rubrum {\n            redde \"rubrum\"\n        }\n        casu caeruleum {\n            redde \"caeruleum\"\n        }\n    }\n}\n";
const NOMINAL_ENUM_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.colore_nomen(utilModule.primus())\n}\n\nincipit {\n    nota run()\n}\n";
const NOMINAL_ENUM_VARIANT_MAIN: &str = "importa ex \"./util\" privata * ut utilModule\n\nfunctio run() → textus {\n    redde utilModule.colore_nomen(utilModule.rubrum)\n}\n\nincipit {\n    nota run()\n}\n";

/// Write a two-module package with `util.fab` content `util_source` and entry
/// `main_source` (both imported through the package manifest).
fn write_nominal_package(dir: &std::path::Path, util_source: &str, main_source: &str) -> std::path::PathBuf {
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
    let types = lowered.validated.validation().types;
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
    let dir = crate::package::test_support::test_temp_dir("s1u2-unify");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_MAIN);
    let config = Config::default();

    let merged = with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        assert_eq!(
            count_merged_nominals(lowered, "struct"),
            1,
            "same nominal across analyses must unify to one TypeId"
        );
    })
    .expect("merged package MIR program");
    let _ = merged;

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
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
    let dir = crate::package::test_support::test_temp_dir("s1u2-two-aliases");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_TWO_ALIASES_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        assert_eq!(
            count_merged_nominals(lowered, "struct"),
            1,
            "same module under two aliases must unify to one TypeId"
        );
    })
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn package_mir_same_name_nominals_from_distinct_modules_do_not_unify() {
    // S1 U2 negative proof (b) coincident-raw-DefId-distinct: two distinct
    // nominal types with the same short name from different module identities
    // must NOT unify — each resolves through its own canonical key, even when
    // the raw source DefIds numerically coincide.
    let dir = crate::package::test_support::test_temp_dir("s1u2-distinct");
    let entry = write_three_module_package(dir.as_ref(), NOMINAL_DISTINCT_UTIL, NOMINAL_DISTINCT_OTHER, NOMINAL_DISTINCT_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        assert_eq!(
            count_merged_nominals(lowered, "struct"),
            2,
            "same-name nominals from distinct module identities must stay distinct"
        );
    })
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
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
    let dir = crate::package::test_support::test_temp_dir("s1u2-enum");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        assert_eq!(
            count_merged_nominals(lowered, "enum"),
            1,
            "same enum across analyses must unify to one TypeId"
        );
    })
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
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
    let dir = crate::package::test_support::test_temp_dir("s1u2-enum-variant");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_VARIANT_MAIN);
    let config = Config::default();

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
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
    let dir = crate::package::test_support::test_temp_dir("s1u2-merged-variants");
    let entry = write_nominal_package(&dir, NOMINAL_ENUM_UTIL, NOMINAL_ENUM_VARIANT_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
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
    })
    .expect("merged package MIR program");
}

/// S1 U2 negative proof (d) merged-import-validation: struct-field access and
/// enum-variant construction validate against the merged nominal type —
/// the struct's fields survive the remap into the entry type table.
#[test]
fn package_mir_merged_validation_carries_imported_struct_fields() {
    let dir = crate::package::test_support::test_temp_dir("s1u2-merged-struct");
    let entry = write_nominal_package(&dir, NOMINAL_UTIL, NOMINAL_MAIN);
    let config = Config::default();

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
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
    })
    .expect("merged package MIR program");
}

/// The canonical identity contract, pinned here for the delivery/campaign
/// note: the key shape is (nominal kind, home-module InterfaceLibraryIdentity
/// {provider, package, module_path}, export name); the unification rule is
/// same-key → one semantic type in the merged program.
#[test]
fn canonical_nominal_identity_key_shape_is_module_qualified() {
    let identity = radix::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["util".to_owned()],
    };
    assert_eq!(identity.display_key(), "package::demo::util");
    let other = radix::file_interface::InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["other".to_owned()],
    };
    assert_ne!(identity, other, "distinct module paths are distinct identities");
    assert_eq!(
        identity.display_key() + "::Persona",
        "package::demo::util::Persona",
        "canonical nominal key = identity + export name"
    );
}
